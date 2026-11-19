import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";
import Waveform from "./Waveform";
import { MOCK_TRACKS, type ReviewState, type Track } from "./mock";
import "./styles.css";

type Channel = {
  id: "L1" | "L2" | "L3";
  name: string;
  desc: string;
  auto: boolean;
};

const CHANNELS: Channel[] = [
  { id: "L1", name: "结果库", desc: "工具自有 JSON / SQLite（本版未接入，不落库）", auto: true },
  { id: "L2", name: "音乐库标签", desc: "BPM / INITIALKEY 字符串标签（与 L3 同步写入）", auto: false },
  { id: "L3", name: "原音频元数据", desc: "TBPM / TKEY 写回原文件，不生成副本", auto: false },
];

const STATE_LABEL: Record<ReviewState, string> = {
  pending: "待复核",
  confirmed: "已确认",
  undetermined: "无法确定",
  skipped: "已跳过",
};

const WAVE_BUCKETS = 480;

type ScanPayload = { root: string; tracks: Record<string, unknown>[]; errors: string[] };
type WaveResult = { buckets: number[]; durationSec: number; sampleRate: number; channels: number; codec: string };
type WaveState = { status: "idle" | "loading" | "ready" | "error"; peaks: number[]; message?: string };

/** Rust analyze_file 的返回（camelCase）。 */
type AnalysisPayload = {
  path: string;
  engine: string;
  bpm: number | null;
  bpmConfidence: number;
  key: string | null;
  keyTop2: string | null;
  keyConfidence: number;
  durationSec: number;
  sampleRate: number;
  elapsedMs: number;
  notes: string[];
};

type EngineInfo = { id: string; label: string; ready: boolean; note: string };

const FALLBACK_ENGINES: EngineInfo[] = [
  { id: "dsp", label: "本地 DSP", ready: true, note: "经典信号处理：拍点自相关 + 调性音阶匹配" },
  { id: "model", label: "神经网络模型", ready: false, note: "需配置外部 runner（model-engine.json）" },
];

function pct(v: number): string {
  if (!Number.isFinite(v)) return "—";
  return Math.round(Math.max(0, Math.min(1, v)) * 100) + "%";
}

function fmtBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "—";
  return (n / 1024 / 1024).toFixed(1) + " MB";
}

function fmtDuration(s: number): string {
  if (!Number.isFinite(s) || s <= 0) return "0:00";
  const m = Math.floor(s / 60);
  const sec = Math.floor(s % 60);
  return m + ":" + String(sec).padStart(2, "0");
}

function stamp(): string {
  return new Date().toLocaleTimeString("zh-CN", { hour12: false });
}

/** Rust TrackFile（camelCase）→ 前端 Track。算法值留空，由既有标签或人工判定补齐。 */
function toTrack(raw: Record<string, unknown>): Track {
  return {
    id: String(raw.path),
    path: String(raw.path),
    fileName: String(raw.fileName ?? ""),
    durationSec: Number(raw.durationSec ?? 0),
    sizeBytes: Number(raw.sizeBytes ?? 0),
    bpmPrimary: 0,
    bpmCandidates: [],
    bpmConfidence: 0,
    keyPrimary: "—",
    keyTop2: [],
    keyConfidence: 0,
    lowConfidence: false,
    bpmExisting: raw.bpmExisting ? String(raw.bpmExisting) : null,
    keyExisting: raw.keyExisting ? String(raw.keyExisting) : null,
  };
}

export default function App() {
  const demo = !isTauri();
  const [tracks, setTracks] = useState<Track[]>(MOCK_TRACKS);
  const [root, setRoot] = useState<string>("");
  const [scanning, setScanning] = useState(false);
  const [selectedId, setSelectedId] = useState<string>(MOCK_TRACKS[0]?.id ?? "");
  const [reviews, setReviews] = useState<Record<string, ReviewState>>(() =>
    Object.fromEntries(MOCK_TRACKS.map((t) => [t.id, "pending" as ReviewState]))
  );
  const [bpmInput, setBpmInput] = useState("");
  const [keyInput, setKeyInput] = useState("");
  const [playingId, setPlayingId] = useState<string | null>(null);
  const [position, setPosition] = useState(0);
  const [clipDuration, setClipDuration] = useState(0);
  const [log, setLog] = useState<string[]>([]);
  const [wave, setWave] = useState<WaveState>({ status: "idle", peaks: [] });
  const [dropActive, setDropActive] = useState(false);
  const [engines, setEngines] = useState<EngineInfo[]>(FALLBACK_ENGINES);
  const [engine, setEngine] = useState<string>("dsp");
  const [analyses, setAnalyses] = useState<Record<string, AnalysisPayload>>({});
  const [analyzeBusy, setAnalyzeBusy] = useState<string | null>(null);
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const writeBusy = useRef(false);
  const waveCache = useRef(new Map<string, number[]>());
  const pendingSeek = useRef<number | null>(null);
  const loadedPath = useRef<string | null>(null);
  const objectUrl = useRef<string | null>(null);

  /** 试听音源：把文件读成 Blob URL。asset 协议在 WebView2 下拿不到数据，这里直接走 IPC。 */
  const attachAudio = async (audio: HTMLAudioElement, path: string) => {
    const bytes = await invoke<ArrayBuffer>("read_audio", { path });
    const ext = path.split(".").pop()?.toLowerCase() ?? "";
    const mime =
      ext === "mp3" ? "audio/mpeg"
        : ext === "m4a" ? "audio/mp4"
          : ext === "flac" ? "audio/flac"
            : ext === "ogg" ? "audio/ogg"
              : ext === "aiff" || ext === "aif" ? "audio/aiff"
                : "audio/wav";
    if (objectUrl.current) URL.revokeObjectURL(objectUrl.current);
    const url = URL.createObjectURL(new Blob([new Uint8Array(bytes)], { type: mime }));
    objectUrl.current = url;
    audio.src = url;
    audio.load();
  };

  const selected: Track | undefined = tracks.find((t) => t.id === selectedId);
  const selectedPath = selected?.path ?? null;
  const realFile = Boolean(selected?.path);

  const addLog = useCallback((line: string) => {
    setLog((prev) => [line, ...prev].slice(0, 12));
  }, []);

  // 切换选中曲目时，写入值预填既有标签；没有则留空等人工判定。
  useEffect(() => {
    if (!selected) return;
    setBpmInput(selected.bpmExisting ?? (selected.bpmPrimary > 0 ? selected.bpmPrimary.toFixed(1) : ""));
    setKeyInput(selected.keyExisting ?? (selected.keyPrimary !== "—" ? selected.keyPrimary : ""));
  }, [selectedId, tracks]); // eslint-disable-line react-hooks/exhaustive-deps

  const counts = useMemo(() => {
    const base = { pending: 0, confirmed: 0, undetermined: 0, skipped: 0 };
    for (const t of tracks) base[reviews[t.id] ?? "pending"] += 1;
    return base;
  }, [reviews, tracks]);

  const stopPlayback = useCallback(() => {
    audioRef.current?.pause();
    if (audioRef.current) audioRef.current.currentTime = 0;
    setPlayingId(null);
    setPosition(0);
    setClipDuration(0);
    pendingSeek.current = null;
  }, []);

  const scanFolder = useCallback(
    async (folder: string | null) => {
      if (demo) {
        addLog(`[${stamp()}] 演示态：浏览器里无法访问本地文件夹，请在 EXE 内使用`);
        return;
      }
      if (!folder) {
        addLog(`[${stamp()}] 未选择文件夹`);
        return;
      }
      setScanning(true);
      try {
        const result = await invoke<ScanPayload>("scan_folder", { root: folder });
        const next = result.tracks.map(toTrack);
        setTracks(next);
        setRoot(result.root);
        setReviews(Object.fromEntries(next.map((t) => [t.id, "pending" as ReviewState])));
        setSelectedId(next[0]?.id ?? "");
        setWave({ status: "idle", peaks: [] });
        stopPlayback();
        addLog(
          `[${stamp()}] 已扫描 ${result.root}：${next.length} 个音频` +
            (result.errors.length ? `，${result.errors.length} 个读取失败` : "")
        );
        for (const err of result.errors.slice(0, 3)) addLog(`  ↳ ${err}`);
      } catch (err) {
        addLog(`[${stamp()}] 扫描失败：${String(err)}`);
      } finally {
        setScanning(false);
      }
    },
    [addLog, demo, stopPlayback]
  );

  const loadFolder = async () => {
    if (demo) {
      addLog(`[${stamp()}] 演示态：浏览器里无法访问本地文件夹，请在 EXE 内使用`);
      return;
    }
    if (scanning) return;
    try {
      const folder = await invoke<string | null>("pick_folder");
      await scanFolder(folder);
    } catch (err) {
      addLog(`[${stamp()}] 选择文件夹失败：${String(err)}`);
    }
  };

  // 拖放：注册一次，内部用 ref 拿最新的扫描函数，避免监听器反复重建。
  const scanRef = useRef(scanFolder);
  scanRef.current = scanFolder;
  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void (async () => {
      try {
        const { getCurrentWebview } = await import("@tauri-apps/api/webview");
        const handle = await getCurrentWebview().onDragDropEvent(async (event) => {
          const payload = event.payload;
          if (payload.type === "over" || payload.type === "enter") {
            setDropActive(true);
            return;
          }
          if (payload.type === "leave") {
            setDropActive(false);
            return;
          }
          if (payload.type === "drop") {
            setDropActive(false);
            const paths = payload.paths ?? [];
            if (paths.length === 0) return;
            const folder = await invoke<string | null>("resolve_drop_folder", { paths });
            if (folder) {
              await scanRef.current(folder);
            } else {
              addLog(`[${stamp()}] 拖入的内容里没有可用的文件夹`);
            }
          }
        });
        if (disposed) handle();
        else unlisten = handle;
      } catch (err) {
        addLog(`[${stamp()}] 拖放监听注册失败：${String(err)}`);
      }
    })();
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [addLog]);

  // 波形：按文件路径缓存，选中即解码；失败只影响波形，不影响试听。
  useEffect(() => {
    if (demo || !selectedPath) {
      setWave({ status: "idle", peaks: [] });
      return;
    }
    const cached = waveCache.current.get(selectedPath);
    if (cached) {
      setWave({ status: "ready", peaks: cached });
      return;
    }
    let alive = true;
    setWave({ status: "loading", peaks: [] });
    invoke<WaveResult>("waveform_peaks", { path: selectedPath, buckets: WAVE_BUCKETS })
      .then((result) => {
        if (!alive) return;
        const peaks = Array.isArray(result.buckets) ? result.buckets : [];
        waveCache.current.set(selectedPath, peaks);
        setWave({ status: "ready", peaks });
      })
      .catch((err) => {
        if (!alive) return;
        setWave({ status: "error", peaks: [], message: String(err) });
      });
    return () => {
      alive = false;
    };
  }, [selectedPath, demo]);

  // 引擎清单从 Rust 读；模型引擎未接入时界面上标灰，不允许选。
  // 带文件夹启动时（命令行第一个参数是目录），启动后自动扫描该文件夹。
  useEffect(() => {
    if (!isTauri()) return;
    void invoke<{ engines?: EngineInfo[]; initialFolder?: string | null }>("app_info")
      .then((info) => {
        if (Array.isArray(info?.engines) && info.engines.length) {
          setEngines(info.engines);
          const ready = info.engines.find((e) => e.ready);
          if (ready) setEngine(ready.id);
        }
        if (info?.initialFolder) {
          void scanRef.current(info.initialFolder);
        }
      })
      .catch(() => {});
  }, []);

  const analysisKey = (path: string, eng: string) => `${path}#${eng}`;

  /** 当前引擎是否可用（模型引擎要配了 runner 才算）。 */
  const engineReady = engines.find((e) => e.id === engine)?.ready ?? true;
  const engineNote = engines.find((e) => e.id === engine)?.note ?? "";

  /** 对单个文件跑一次分析。只产出候选，不写任何标签。 */
  const runAnalysis = useCallback(
    async (track: Track) => {
      if (demo || !track.path) {
        addLog(`[${stamp()}] 演示数据没有真实音频，无法分析`);
        return;
      }
      if (engineReady === false) {
        addLog(`[${stamp()}] ${engine.toUpperCase()} 引擎不可用：${engineNote}`);
        return;
      }
      const path = track.path;
      setAnalyzeBusy(path);
      try {
        const result = await invoke<AnalysisPayload>("analyze_file", {
          path,
          engine,
        });
        setAnalyses((prev) => ({ ...prev, [analysisKey(path, result.engine)]: result }));
        const bpmText = result.bpm !== null ? `${result.bpm.toFixed(1)} BPM` : "BPM 未定";
        const keyText = result.key ?? "调性未定";
        addLog(
          `[${stamp()}] ${track.fileName} → ${result.engine.toUpperCase()} 分析：${bpmText} / ${keyText}（${result.elapsedMs} ms）`
        );
      } catch (err) {
        addLog(`[${stamp()}] ${track.fileName} → 分析失败：${String(err)}`);
      } finally {
        setAnalyzeBusy(null);
      }
    },
    [addLog, demo, engine, engineReady, engineNote]
  );

  const analyzeAll = async () => {
    if (demo) {
      addLog(`[${stamp()}] 演示态：没有真实文件可分析`);
      return;
    }
    if (engineReady === false) {
      addLog(`[${stamp()}] ${engine.toUpperCase()} 引擎不可用：${engineNote}`);
      return;
    }
    const targets = tracks.filter((t) => t.path);
    if (targets.length === 0) return;
    addLog(`[${stamp()}] 开始批量分析 ${targets.length} 个文件（${engine.toUpperCase()}）`);
    for (const t of targets) {
      await runAnalysis(t);
    }
    addLog(`[${stamp()}] 批量分析结束`);
  };

  const togglePlay = (track: Track) => {
    if (demo || !track.path) {
      addLog(`[${stamp()}] 演示数据无真实音频可试听`);
      return;
    }
    const audio = audioRef.current;
    if (!audio) return;
    const path: string = track.path!;
    if (playingId === track.id) {
      audio.pause();
      setPlayingId(null);
      return;
    }
    void (async () => {
      try {
        if (loadedPath.current !== path) {
          await attachAudio(audio, path);
          loadedPath.current = path;
          pendingSeek.current = null;
          audio.currentTime = 0;
          setPosition(0);
          setClipDuration(0);
        }
        await audio.play();
        setPlayingId(track.id);
      } catch (err) {
        addLog(`[${stamp()}] 试听失败：${String(err)}`);
      }
    })();
  };

  /** 波形点选 / 拖动：ratio 0..1，play 表示是否同时开始播放。 */
  const seekByRatio = (ratio: number, play: boolean) => {
    if (demo || !selected?.path) return;
    const audio = audioRef.current;
    if (!audio) return;
    const sel = selected;
    const path: string = sel.path!;
    const target = Math.min(1, Math.max(0, ratio));
    void (async () => {
      try {
        if (loadedPath.current !== path) {
          loadedPath.current = path;
          pendingSeek.current = target;
          await attachAudio(audio, path);
          setClipDuration(sel.durationSec);
          if (play) {
            await audio.play();
            setPlayingId(sel.id);
          }
          return;
        }
        const total = audio.duration;
        if (Number.isFinite(total) && total > 0) {
          const nextTime = target * total;
          audio.currentTime = nextTime;
          setPosition(nextTime);
        }
        if (play && playingId !== selected.id) {
          await audio.play();
          setPlayingId(selected.id);
        }
      } catch (err) {
        addLog(`[${stamp()}] 试听失败：${String(err)}`);
      }
    })();
  };

  const decide = async (next: ReviewState) => {
    if (!selected || writeBusy.current) return;
    if (next !== "confirmed") {
      setReviews((prev) => ({ ...prev, [selected.id]: next }));
      addLog(
        next === "undetermined"
          ? `[${stamp()}] ${selected.fileName} → 标记为无法确定，三通道均不写入`
          : `[${stamp()}] ${selected.fileName} → 已跳过，不写入`
      );
      return;
    }
    if (demo || !selected.path) {
      setReviews((prev) => ({ ...prev, [selected.id]: "confirmed" }));
      addLog(`[${stamp()}] ${selected.fileName} → 复核通过，演示态不触碰真实文件`);
      return;
    }
    writeBusy.current = true;
    try {
      const result = await invoke<{
        bpmWritten: string | null;
        keyWritten: string | null;
        bpmBefore: string | null;
        keyBefore: string | null;
      }>("write_metadata", { request: { path: selected.path, bpm: bpmInput, key: keyInput } });
      setReviews((prev) => ({ ...prev, [selected.id]: "confirmed" }));
      const written = [
        result.bpmWritten ? `BPM=${result.bpmWritten}` : null,
        result.keyWritten ? `Key=${result.keyWritten}` : null,
      ]
        .filter(Boolean)
        .join("、");
      addLog(`[${stamp()}] ${selected.fileName} → 已写回原文件（${written}）`);
      if (result.bpmBefore || result.keyBefore) {
        addLog(`  ↳ 写前旧值：${[result.bpmBefore, result.keyBefore].filter(Boolean).join(" / ") || "（空）"}`);
      }
      // 回读后同步既有标签显示
      setTracks((prev) =>
        prev.map((t) =>
          t.id === selected.id
            ? { ...t, bpmExisting: result.bpmWritten ?? t.bpmExisting, keyExisting: result.keyWritten ?? t.keyExisting }
            : t
        )
      );
      stopPlayback();
    } catch (err) {
      addLog(`[${stamp()}] ${selected.fileName} → 写入失败：${String(err)}`);
    } finally {
      writeBusy.current = false;
    }
  };

  const waveDuration = clipDuration || selected?.durationSec || 0;
  const waveProgress = playingId === selected?.id && waveDuration > 0 ? Math.min(1, position / waveDuration) : 0;
  const engineLabel = engines.find((e) => e.id === engine)?.label ?? "本地 DSP";
  const selectedAnalysis = selected?.path ? analyses[analysisKey(selected.path, engine)] : undefined;

  return (
    <div className={"app" + (dropActive ? " app-drop" : "")}>
      <header className="topbar">
        <div className="brand">
          <img className="logo" src="/icons/brand.png" alt="BPM Key Tools" />
          <div>
            <h1>BPM 调性识别</h1>
          </div>
        </div>
        <span className="badge badge-warn">
          {demo
            ? "演示态 · 无法访问真实文件"
            : `本机 ${engineLabel} 分析 · 候选需复核后写回`}
        </span>
      </header>

      <div className="body">
        <aside className="list">
          <div className="list-head">
            <span>
              {root ? root.split(/[\\/]/).pop() || root : demo ? "演示候选" : "未加载"}
            </span>
            <span className="muted">待复核 {counts.pending}</span>
          </div>
          <div className="list-toolbar">
            <button className="btn btn-primary" onClick={() => void loadFolder()} disabled={scanning}>
              {scanning ? "扫描中…" : tracks.length && root ? "重新选择文件夹" : "选择文件夹"}
            </button>
            <button
              className="btn btn-sm"
              onClick={() => void analyzeAll()}
              disabled={demo || Boolean(analyzeBusy) || tracks.length === 0}
              title={demo ? "演示数据无真实音频" : `对列表内 ${tracks.length} 个文件依次分析`}
            >
              {analyzeBusy ? "分析中…" : "分析全部"}
            </button>
            <span className="muted">{tracks.length} 个音频</span>
          </div>
          {!demo && <div className="list-hint">也可以把文件夹直接拖进窗口</div>}
          <ul>
            {tracks.map((t) => (
              <li key={t.id} className={"row-wrap" + (t.id === selectedId ? " row-active-wrap" : "")}>
                <button
                  className="play-btn"
                  aria-label={playingId === t.id ? `暂停 ${t.fileName}` : `试听 ${t.fileName}`}
                  title={t.path ? "试听" : "演示数据，无真实音频"}
                  onClick={() => togglePlay(t)}
                >
                  {playingId === t.id ? "⏸" : "▶"}
                </button>
                <button
                  className={"row" + (t.id === selectedId ? " row-active" : "")}
                  onClick={() => setSelectedId(t.id)}
                >
                  <span className="row-name">{t.fileName}</span>
                  <span className="row-meta">
                    {fmtDuration(t.durationSec)} · {fmtBytes(t.sizeBytes)}
                    {t.bpmExisting ? ` · ${t.bpmExisting} BPM` : ""}
                    {t.keyExisting ? ` · ${t.keyExisting}` : ""}
                  </span>
                  <span className={"chip chip-" + (reviews[t.id] ?? "pending")}>
                    {STATE_LABEL[reviews[t.id] ?? "pending"]}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </aside>

        <main className="detail">
          {selected ? (
            <>
              <div className="detail-head">
                <h2>{selected.fileName}</h2>
                <span className={"chip chip-" + (reviews[selected.id] ?? "pending")}>
                  {STATE_LABEL[reviews[selected.id] ?? "pending"]}
                </span>
              </div>

              {!realFile && (
                <div className="notice">
                  演示数据：仅供复核流程走查，试听与写回需在 EXE 内加载真实文件夹。
                </div>
              )}

              <section className="player-block">
                <button
                  className="btn play-big"
                  onClick={() => togglePlay(selected)}
                  disabled={!realFile}
                  title={realFile ? "试听原文件" : "演示数据无真实音频"}
                >
                  {playingId === selected.id ? "⏸ 暂停" : "▶ 试听"}
                </button>

                <div className="wave-zone">
                  {wave.status === "ready" && wave.peaks.length > 0 ? (
                    <Waveform
                      peaks={wave.peaks}
                      progress={waveProgress}
                      onSeek={seekByRatio}
                      disabled={!realFile}
                      height={72}
                    />
                  ) : (
                    <div className={"wave-placeholder" + (wave.status === "error" ? " wave-error" : "")}>
                      {wave.status === "loading"
                        ? "波形解码中…"
                        : wave.status === "error"
                          ? `波形不可用：${wave.message ?? "解码失败"}`
                          : realFile
                            ? "暂无波形"
                            : "演示数据没有真实音频，无法预览波形"}
                    </div>
                  )}
                  <div className="wave-axis">
                    <span className="num">{fmtDuration(playingId === selected.id ? position : 0)}</span>
                    <span className="wave-tip">点波形跳选，按住拖动可连续定位</span>
                    <span className="num">
                      {fmtDuration(clipDuration || selected.durationSec)}
                    </span>
                  </div>
                </div>
              </section>

              <section className="analysis">
                <div className="analysis-head">
                  <span className="analysis-title">算法候选</span>
                  <select
                    className="select"
                    value={engine}
                    onChange={(e) => setEngine(e.target.value)}
                    aria-label="分析引擎"
                  >
                    {engines.map((en) => (
                      <option key={en.id} value={en.id} disabled={!en.ready}>
                        {en.label}
                        {en.ready ? "" : "（未配置）"}
                      </option>
                    ))}
                  </select>
                  <button
                    className="btn btn-sm btn-primary"
                    onClick={() => void runAnalysis(selected)}
                    disabled={!realFile || analyzeBusy === selected.path || !engineReady}
                  >
                    {analyzeBusy === selected.path ? "分析中…" : "运行分析"}
                  </button>
                </div>
                {!engineReady && (
                  <p className="analysis-hint">{engineNote}</p>
                )}

                {selectedAnalysis ? (
                  <>
                    <div className="analysis-grid">
                      <div className="a-item">
                        <span className="a-label">BPM</span>
                        <b className="a-value">
                          {selectedAnalysis.bpm !== null ? selectedAnalysis.bpm.toFixed(1) : "未定"}
                        </b>
                        <span className="a-sub">置信 {pct(selectedAnalysis.bpmConfidence)}</span>
                        {selectedAnalysis.bpm !== null && (
                          <button
                            className="link"
                            onClick={() => setBpmInput(selectedAnalysis.bpm!.toFixed(1))}
                          >
                            采用
                          </button>
                        )}
                      </div>
                      <div className="a-item">
                        <span className="a-label">调性</span>
                        <b className="a-value">{selectedAnalysis.key ?? "未定"}</b>
                        <span className="a-sub">置信 {pct(selectedAnalysis.keyConfidence)}</span>
                        {selectedAnalysis.keyTop2 && (
                          <span className="a-sub">次优 {selectedAnalysis.keyTop2}</span>
                        )}
                        {selectedAnalysis.key && (
                          <button className="link" onClick={() => setKeyInput(selectedAnalysis.key!)}>
                            采用
                          </button>
                        )}
                      </div>
                      <div className="a-item">
                        <span className="a-label">耗时</span>
                        <b className="a-value">{selectedAnalysis.elapsedMs} ms</b>
                        <span className="a-sub">
                          {selectedAnalysis.durationSec.toFixed(1)} s · {selectedAnalysis.sampleRate} Hz
                        </span>
                      </div>
                    </div>
                    {selectedAnalysis.notes.length > 0 && (
                      <ul className="note-list">
                        {selectedAnalysis.notes.slice(0, 3).map((n, i) => (
                          <li key={i}>{n}</li>
                        ))}
                      </ul>
                    )}
                  </>
                ) : (
                  <div className="analysis-hint">
                    {demo
                      ? "演示数据不含真实音频，分析不可用。"
                      : "分析只产出候选值：点「采用」填进下方输入框，仍需你确认后才写回原文件。"}
                  </div>
                )}
              </section>

              <section className="cards">
                <div className="card">
                  <div className="card-label">BPM（复核后写回）</div>
                  <input
                    className="field"
                    placeholder="如 120.0；留空则不写"
                    value={bpmInput}
                    onChange={(e) => setBpmInput(e.target.value)}
                  />
                  <div className="card-sub">
                    既有标签：{selected.bpmExisting ?? "无"}
                    {selectedAnalysis?.bpm != null
                      ? `；候选 ${selectedAnalysis.bpm.toFixed(1)}（置信 ${pct(selectedAnalysis.bpmConfidence)}）`
                      : "；尚无候选"}
                  </div>
                </div>

                <div className="card">
                  <div className="card-label">调性（复核后写回）</div>
                  <input
                    className="field"
                    placeholder="如 A minor；留空则不写"
                    value={keyInput}
                    onChange={(e) => setKeyInput(e.target.value)}
                  />
                  <div className="card-sub">
                    既有标签：{selected.keyExisting ?? "无"}
                    {selectedAnalysis?.key
                      ? `；候选 ${selectedAnalysis.key}（置信 ${pct(selectedAnalysis.keyConfidence)}）`
                      : "；尚无候选"}
                  </div>
                </div>
              </section>

              <section className="actions">
                <button
                  className="btn btn-primary"
                  onClick={() => void decide("confirmed")}
                  disabled={!bpmInput.trim() && !keyInput.trim()}
                >
                  确认并写入
                </button>
                <button className="btn" onClick={() => void decide("undetermined")}>
                  标记无法确定
                </button>
                <button className="btn btn-ghost" onClick={() => void decide("skipped")}>
                  跳过
                </button>
              </section>

              <section className="channels">
                <h3>写入通道</h3>
                <ul>
                  {CHANNELS.map((c) => {
                    const confirmed = reviews[selected.id] === "confirmed";
                    const state = c.auto
                      ? "本版未接入"
                      : confirmed && realFile
                        ? "已写回原文件"
                        : confirmed
                          ? "演示态 · 未写真实文件"
                          : "未复核 · 不写入";
                    return (
                      <li key={c.id}>
                        <span className="ch-id">{c.id}</span>
                        <span className="ch-name">{c.name}</span>
                        <span className="ch-desc">{c.desc}</span>
                        <span className={"ch-state " + (confirmed && realFile && !c.auto ? "on" : "off")}>
                          {state}
                        </span>
                      </li>
                    );
                  })}
                </ul>
              </section>
            </>
          ) : (
            <div className="empty">没有候选文件。点左侧「选择文件夹」加载音频。</div>
          )}
        </main>
      </div>

      <footer className="log">
        <div className="log-head">操作记录{demo ? "（演示态，不触碰真实文件）" : "（写回均为真实文件操作）"}</div>
        <ol>
          {log.length === 0 ? (
            <li className="muted">尚无操作。</li>
          ) : (
            log.map((l, i) => <li key={i}>{l}</li>)
          )}
        </ol>
      </footer>

      <footer className="statusbar">
        <span className="status-version">BPM 调性识别 | v0.4.1</span>
      </footer>

      {dropActive && (
        <div className="drop-overlay">
          <div className="drop-card">
            <div className="drop-title">松开即可加载文件夹</div>
            <div className="drop-sub">递归扫描 WAV / AIFF / FLAC / MP3 / M4A / OGG / Opus</div>
          </div>
        </div>
      )}

      <audio
        ref={audioRef}
        hidden
        onTimeUpdate={(e) => setPosition(e.currentTarget.currentTime)}
        onLoadedMetadata={(e) => {
          const audio = e.currentTarget;
          setClipDuration(audio.duration || 0);
          if (pendingSeek.current !== null && Number.isFinite(audio.duration) && audio.duration > 0) {
            const target = pendingSeek.current * audio.duration;
            audio.currentTime = target;
            setPosition(target);
            pendingSeek.current = null;
          }
        }}
        onEnded={() => {
          setPlayingId(null);
          setPosition(0);
        }}
      />
    </div>
  );
}
