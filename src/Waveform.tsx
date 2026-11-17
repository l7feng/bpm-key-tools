import { useEffect, useRef, useState } from "react";

type Props = {
  /** 归一化 0..1 的峰值序列。 */
  peaks: number[];
  /** 0..1 的播放进度；<=0 时不画播放头。 */
  progress: number;
  /** 点波形 / 拖动波形：ratio 为 0..1，play 表示这一次交互要不要顺带播放。 */
  onSeek?: (ratio: number, play: boolean) => void;
  disabled?: boolean;
  height?: number;
};

/**
 * 「试听」旁边的波形预览：柱状峰值 + 已播放部分高亮 + 可点击 / 拖动的播放头。
 * 只负责画，不解码：峰值由 Rust 侧 waveform_peaks 计算后传进来。
 */
export default function Waveform({ peaks, progress, onSeek, disabled = false, height = 72 }: Props) {
  const wrapRef = useRef<HTMLDivElement | null>(null);
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const draggingRef = useRef(false);
  const [width, setWidth] = useState(0);

  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    setWidth(el.clientWidth);
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) setWidth(entry.contentRect.width);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || width <= 0) return;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    canvas.style.width = `${width}px`;
    canvas.style.height = `${height}px`;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, width, height);

    // 中线：波形上下对称，中线弱化，避免整块看起来像柱状图。
    ctx.fillStyle = "rgba(106, 169, 224, 0.16)";
    ctx.fillRect(0, height / 2 - 0.5, width, 1);

    const count = peaks.length;
    if (count === 0) return;
    const slot = width / count;
    const barWidth = Math.max(1, Math.min(3, slot * 0.74));
    const usable = height - 6;
    const clampedProgress = Math.min(1, Math.max(0, progress));

    for (let index = 0; index < count; index += 1) {
      const value = Math.min(1, Math.max(0.02, peaks[index] ?? 0));
      const barHeight = Math.max(1, value * usable);
      const x = index * slot + (slot - barWidth) / 2;
      const y = (height - barHeight) / 2;
      const played = clampedProgress > 0 && (index + 0.5) / count <= clampedProgress;
      ctx.fillStyle = played ? "#8fc2ec" : "rgba(106, 169, 224, 0.34)";
      ctx.fillRect(x, y, barWidth, barHeight);
    }

    if (clampedProgress > 0) {
      const x = clampedProgress * width;
      ctx.fillStyle = "#ece9d9";
      ctx.fillRect(Math.max(0, Math.min(width - 2, x - 1)), 2, 2, height - 4);
    }
  }, [peaks, progress, width, height]);

  const ratioOf = (clientX: number, rect: DOMRect) => {
    if (rect.width <= 0) return 0;
    return Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
  };

  return (
    <div
      ref={wrapRef}
      className={"waveform" + (disabled ? " waveform-disabled" : "")}
      style={{ height }}
      role="slider"
      aria-label="波形播放位置"
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={Math.round(Math.min(1, Math.max(0, progress)) * 100)}
      tabIndex={disabled ? -1 : 0}
      onPointerDown={(event) => {
        if (disabled || !onSeek) return;
        const rect = event.currentTarget.getBoundingClientRect();
        draggingRef.current = true;
        event.currentTarget.setPointerCapture(event.pointerId);
        onSeek(ratioOf(event.clientX, rect), true);
      }}
      onPointerMove={(event) => {
        if (disabled || !onSeek || !draggingRef.current) return;
        const rect = event.currentTarget.getBoundingClientRect();
        onSeek(ratioOf(event.clientX, rect), false);
      }}
      onPointerUp={(event) => {
        draggingRef.current = false;
        if (event.currentTarget.hasPointerCapture(event.pointerId)) {
          event.currentTarget.releasePointerCapture(event.pointerId);
        }
      }}
      onPointerCancel={() => {
        draggingRef.current = false;
      }}
      onKeyDown={(event) => {
        if (disabled || !onSeek) return;
        if (event.key === "ArrowLeft" || event.key === "ArrowRight") {
          event.preventDefault();
          const step = event.key === "ArrowLeft" ? -0.02 : 0.02;
          onSeek(Math.min(1, Math.max(0, progress + step)), false);
        }
      }}
    >
      <canvas ref={canvasRef} className="waveform-canvas" />
    </div>
  );
}
