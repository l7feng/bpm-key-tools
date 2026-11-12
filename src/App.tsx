import { useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc, invoke, isTauri } from "@tauri-apps/api/core";
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

function fmtBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "—";
  return (n / 1024 / 1024).toFixed(1) + " MB";
}

function fmtDuration(s: number): string {
  if (!Number.isFinite(s) || s <= 0) return "—";
  const m = Math.floor(s / 60);
  const sec = Math.round(s % 60);
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
  const audioRef = useRef<HTMLAudioElement | null>(null);
  const writeBusy = useRef(false);

  const selected: Track | undefined = tracks.find((t) => t.id === selectedId);
  const realFile = Boolean(selected?.path);

  // 切换选中曲目时，写入值预填既有标签；没有则留空等人工判定。
  useEffect(() => {
    if (!selected) return;
    setBpmInput(selected.bpmExisting ?? (selected.bpmPrimary > 0 ? selected.bpmPrimary.toFixed(1) : ""));
    setKeyInput(selected.keyExisting ?? (selected.keyPrimary !== "—" ? selected.keyPrimary : ""));
  }, [selectedId, tracks]); // eslint-disable-line react-hooks/exhaustive-deps

  const addLog = (line: string) => setLog((prev) => [line, ...prev].slice(0, 12));

  const counts = useMemo(() => {
    const base = { pending: 0, confirmed: 0, undetermined: 0, skipped: 0 };
    for (const t of tracks) base[reviews[t.id] ?? "pending"] += 1;
    return base;
  }, [reviews, tracks]);

  const loadFolder = async () => {
    if (demo || scanning) {
      if (demo) addLog(`[${stamp()}] 演示态：浏览器里无法访问本地文件夹，请在 EXE 内使用`);
      return;
    }
    setScanning(true);
    try {
      const folder = await invoke<string | null>("pick_folder");
      if (!folder) {
        addLog(`[${stamp()}] 未选择文件夹`);
        return;
      }
      const result = await invoke<{ root: string; tracks: Record<string, unknown>[]; errors: string[] }>(
        "scan_folder",
        { root: folder }
      );
      const next = result.tracks.map(toTrack);
      setTracks(next);
      setRoot(result.root);
      setReviews(Object.fromEntries(next.map((t) => [t.id, "pending" as ReviewState])));
      setSelectedId(next[0]?.id ?? "");
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
  };

  const stopPlayback = () => {
    audioRef.current?.pause();
    if (audioRef.current) audioRef.current.currentTime = 0;
    setPlayingId(null);
    setPosition(0);
    setClipDuration(0);
  };

  const togglePlay = (track: Track) => {
    if (demo || !track.path) {
      addLog(`[${stamp()}] 演示数据无真实音频可试听`);
      return;
    }
    const audio = audioRef.current;
    if (!audio) return;
    if (playingId === track.id) {
      audio.pause();
      setPlayingId(null);
      return;
    }
    audio.src = convertFileSrc(track.path);
    audio.currentTime = 0;
    void audio.play().then(() => setPlayingId(track.id));
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

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <img className="logo" src="/icons/logo.png" alt="" aria-hidden="true" />
          <div>
            <h1>BPM 调性识别</h1>
            <p className="sub">v0.2.0 · 复核闸门（真实文件试听与元数据写回）</p>
          </div>
        </div>
        <span className="badge badge-warn">
          {demo
            ? "演示态 · 无法访问真实文件"
            : "算法未接入 · BPM/调性值来自既有标签或人工判定"}
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
            <span className="muted">{tracks.length} 个音频</span>
          </div>
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
                <span className="player-time">
                  {fmtDuration(playingId === selected.id || position ? position : 0)} /{" "}
                  {clipDuration && playingId === selected.id ? fmtDuration(clipDuration) : fmtDuration(selected.durationSec)}
                </span>
                <input
                  type="range"
                  className="seek"
                  min={0}
                  max={clipDuration || selected.durationSec || 0}
                  step={0.1}
                  value={playingId === selected.id ? position : 0}
                  disabled={!realFile}
                  aria-label="试听进度"
                  onChange={(e) => {
                    const audio = audioRef.current;
                    if (audio) audio.currentTime = Number(e.target.value);
                  }}
                />
              </section>

              <section className="cards">
                <div className="card">
                  <div className="card-label">BPM（既有标签或人工判定）</div>
                  <input
                    className="field"
                    placeholder="如 120.0；留空则不写"
                    value={bpmInput}
                    onChange={(e) => setBpmInput(e.target.value)}
                  />
                  <div className="card-sub">
                    既有标签：{selected.bpmExisting ?? "无"}；算法候选未接入。
                  </div>
                </div>

                <div className="card">
                  <div className="card-label">调性（既有标签或人工判定）</div>
                  <input
                    className="field"
                    placeholder="如 A minor；留空则不写"
                    value={keyInput}
                    onChange={(e) => setKeyInput(e.target.value)}
                  />
                  <div className="card-sub">
                    既有标签：{selected.keyExisting ?? "无"}；算法候选未接入。
                  </div>
                </div>
              </section>

              <section className="actions">
                <button className="btn btn-primary" onClick={() => void decide("confirmed")} disabled={!bpmInput.trim() && !keyInput.trim()}>
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

      <audio
        ref={audioRef}
        hidden
        onTimeUpdate={(e) => setPosition(e.currentTarget.currentTime)}
        onLoadedMetadata={(e) => setClipDuration(e.currentTarget.duration || 0)}
        onEnded={() => {
          setPlayingId(null);
          setPosition(0);
        }}
      />
    </div>
  );
}
