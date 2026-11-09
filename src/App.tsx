import { useMemo, useState } from "react";
import { MOCK_TRACKS, type ReviewState, type Track } from "./mock";
import "./styles.css";

type Channel = {
  id: "L1" | "L2" | "L3";
  name: string;
  desc: string;
  auto: boolean;
};

const CHANNELS: Channel[] = [
  { id: "L1", name: "结果库", desc: "工具自有 JSON / SQLite，复核前即可写", auto: true },
  { id: "L2", name: "音乐库标签", desc: "BPM: / 调性: 字符串标签", auto: false },
  { id: "L3", name: "原音频元数据", desc: "iXML / ID3，每批留可撤销副本", auto: false },
];

const STATE_LABEL: Record<ReviewState, string> = {
  pending: "待复核",
  confirmed: "已确认",
  undetermined: "无法确定",
  skipped: "已跳过",
};

function fmtBytes(n: number): string {
  return (n / 1024 / 1024).toFixed(1) + " MB";
}

function fmtDuration(s: number): string {
  const m = Math.floor(s / 60);
  const sec = Math.round(s % 60);
  return m + ":" + String(sec).padStart(2, "0");
}

export default function App() {
  const [selectedId, setSelectedId] = useState<string>(MOCK_TRACKS[0]?.id ?? "");
  const [reviews, setReviews] = useState<Record<string, ReviewState>>(() =>
    Object.fromEntries(MOCK_TRACKS.map((t) => [t.id, "pending" as ReviewState]))
  );
  const [log, setLog] = useState<string[]>([]);

  const selected: Track | undefined = MOCK_TRACKS.find((t) => t.id === selectedId);

  const counts = useMemo(() => {
    const base = { pending: 0, confirmed: 0, undetermined: 0, skipped: 0 };
    for (const t of MOCK_TRACKS) base[reviews[t.id]] += 1;
    return base;
  }, [reviews]);

  const decide = (next: ReviewState) => {
    if (!selected) return;
    setReviews((prev) => ({ ...prev, [selected.id]: next }));
    const stamp = new Date().toLocaleTimeString("zh-CN", { hour12: false });
    const line =
      next === "confirmed"
        ? `[${stamp}] ${selected.fileName} → 复核通过，模拟写入 L2 标签与 L3 元数据（未触碰真实文件）`
        : next === "undetermined"
          ? `[${stamp}] ${selected.fileName} → 标记为无法确定，三通道均不写入`
          : `[${stamp}] ${selected.fileName} → 已跳过，不写入`;
    setLog((prev) => [line, ...prev].slice(0, 12));
  };

  return (
    <div className="app">
      <header className="topbar">
        <div className="brand">
          <span className="logo" aria-hidden="true">
            ♩
          </span>
          <div>
            <h1>BPM 调性识别</h1>
            <p className="sub">v0.1.0 · 复核闸门骨架</p>
          </div>
        </div>
        <span className="badge badge-warn">算法未接入 · 演示数据</span>
      </header>

      <div className="body">
        <aside className="list">
          <div className="list-head">
            <span>候选文件 {MOCK_TRACKS.length}</span>
            <span className="muted">待复核 {counts.pending}</span>
          </div>
          <ul>
            {MOCK_TRACKS.map((t) => (
              <li key={t.id}>
                <button
                  className={"row" + (t.id === selectedId ? " row-active" : "")}
                  onClick={() => setSelectedId(t.id)}
                >
                  <span className="row-name">{t.fileName}</span>
                  <span className="row-meta">
                    {fmtDuration(t.durationSec)} · {fmtBytes(t.sizeBytes)}
                  </span>
                  <span className={"chip chip-" + reviews[t.id]}>{STATE_LABEL[reviews[t.id]]}</span>
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
                <span className={"chip chip-" + reviews[selected.id]}>
                  {STATE_LABEL[reviews[selected.id]]}
                </span>
              </div>

              {selected.lowConfidence && (
                <div className="notice">
                  置信度低于阈值，建议人工试听后判定，或直接标记为无法确定。
                </div>
              )}

              <section className="cards">
                <div className="card">
                  <div className="card-label">BPM 主值</div>
                  <div className="card-value">
                    {selected.bpmPrimary > 0 ? selected.bpmPrimary.toFixed(1) : "—"}
                  </div>
                  <div className="card-sub">
                    候选：{selected.bpmCandidates.length ? selected.bpmCandidates.join(" / ") : "无"}
                  </div>
                  <div className="card-sub">置信度 {selected.bpmConfidence.toFixed(2)}</div>
                </div>

                <div className="card">
                  <div className="card-label">调性主选</div>
                  <div className="card-value">{selected.keyPrimary}</div>
                  <div className="card-sub">
                    次选：{selected.keyTop2.slice(1).join(" / ") || "无"}
                  </div>
                  <div className="card-sub">置信度 {selected.keyConfidence.toFixed(2)}</div>
                </div>
              </section>

              <section className="actions">
                <button className="btn btn-primary" onClick={() => decide("confirmed")}>
                  确认并写入
                </button>
                <button className="btn" onClick={() => decide("undetermined")}>
                  标记无法确定
                </button>
                <button className="btn btn-ghost" onClick={() => decide("skipped")}>
                  跳过
                </button>
              </section>

              <section className="channels">
                <h3>写入通道</h3>
                <ul>
                  {CHANNELS.map((c) => {
                    const confirmed = reviews[selected.id] === "confirmed";
                    const state = c.auto
                      ? "复核前可写"
                      : confirmed
                        ? "已模拟写入"
                        : "未复核 · 不写入";
                    return (
                      <li key={c.id}>
                        <span className="ch-id">{c.id}</span>
                        <span className="ch-name">{c.name}</span>
                        <span className="ch-desc">{c.desc}</span>
                        <span className={"ch-state " + (confirmed && !c.auto ? "on" : "off")}>
                          {state}
                        </span>
                      </li>
                    );
                  })}
                </ul>
              </section>
            </>
          ) : (
            <div className="empty">没有候选文件。</div>
          )}
        </main>
      </div>

      <footer className="log">
        <div className="log-head">操作记录（模拟，不触碰真实文件）</div>
        <ol>
          {log.length === 0 ? (
            <li className="muted">尚无操作。</li>
          ) : (
            log.map((l, i) => <li key={i}>{l}</li>)
          )}
        </ol>
      </footer>
    </div>
  );
}
