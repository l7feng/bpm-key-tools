#!/usr/bin/env python3
"""BPM Key Tools 模型引擎 runner（参考实现）。

契约（Rust 侧 model_engine.rs 只认这一份）：
  调用： <exe> model_runner.py --audio <解码后的 22.05 kHz 单声道 WAV> --config <model-engine.json>
  输出： stdout 最后一行 JSON，字段名 camelCase：
        {"bpm": 120.0|null, "bpmConfidence": 0.0-1.0,
         "key": "A minor"|null, "keyConfidence": 0.0-1.0,
         "keyTop2": "C major"|null, "model": "…", "notes": ["…"]}
  约定：置信度不够就输出 null，不猜；stderr 只放诊断信息。

依赖：numpy + onnxruntime（本机 sfx-tools-ai 环境两者都有）。
不依赖 librosa：mel 滤波器组在这里自己算。

模型目录（配置里的 modelsDir）需要：
  spec.json        —— 前后端约定（见 engine/spec.example.json）
  若干 .onnx       —— 权重，由使用者自行放置（许可随模型走，不随本仓库分发）
"""

from __future__ import annotations

import argparse
import json
import math
import os
import sys
import wave

import numpy as np

KEY_NAMES = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
]

# Krumhansl-Kessler 大/小调音级权重（索引＝相对主音的音级）
MAJOR_WEIGHTS = [6.35, 2.23, 3.48, 2.05, 4.19, 2.20, 3.17, 2.79, 2.22, 4.62, 2.36, 2.09]
MINOR_WEIGHTS = [6.40, 2.71, 2.98, 5.28, 2.23, 3.66, 2.32, 2.50, 3.36, 2.40, 3.53, 2.07]


def cosine(a: np.ndarray, b: np.ndarray) -> float:
    na = float(np.linalg.norm(a))
    nb = float(np.linalg.norm(b))
    if na < 1e-9 or nb < 1e-9:
        return 0.0
    return float(np.dot(a, b) / (na * nb))


def key_from_chroma(chroma: np.ndarray) -> tuple[str | None, str | None, float]:
    """12 音级 chroma → 调性 top1/top2 与置信度（与 DSP 内核同一套口径）。"""
    chroma = chroma / (chroma.sum() + 1e-9)
    majors = np.asarray(MAJOR_WEIGHTS, dtype=np.float64)
    minors = np.asarray(MINOR_WEIGHTS, dtype=np.float64)
    scores = []
    for root in range(12):
        rotated = np.roll(chroma, -root)
        scores.append((cosine(rotated, majors), f"{KEY_NAMES[root]} major"))
        scores.append((cosine(rotated, minors), f"{KEY_NAMES[root]} minor"))
    scores.sort(key=lambda x: x[0], reverse=True)
    best_score, best = scores[0]
    second_score, second = scores[1]
    concentration = float(np.max(chroma))
    if concentration < 0.07:
        return None, None, 0.0
    if best_score <= 0:
        return None, None, 0.0
    conf = (best_score - second_score) / best_score * math.sqrt(concentration)
    return best, second, max(0.0, min(1.0, conf))


def tempo_from_envelope(env: np.ndarray, frame_rate: float) -> tuple[float | None, float]:
    """NN 起音包络 → 自相关 + 谐波叠加 + 抛物线插值，返回 (BPM, 峰值)。"""
    if env.size < 16:
        return None, 0.0
    env = env - env.mean()
    if env.std() < 1e-6:
        return None, 0.0
    env = env / env.std()
    n = env.size
    ac = np.correlate(env, env, mode="full")[n - 1 :]
    ac = ac / (np.arange(n, 0, -1))  # 按重叠长度归一，避免长 lag 天然偏大
    lag_min = int(frame_rate * 60.0 / 210.0)
    lag_max = min(int(frame_rate * 60.0 / 40.0), n - 2)
    if lag_max <= lag_min + 2:
        return None, 0.0
    best_lag, best_score = 0, -1e9
    for lag in range(lag_min, lag_max + 1):
        score = ac[lag]
        if lag * 2 < n:
            score += 0.5 * ac[lag * 2]
        if lag * 3 < n:
            score += 0.33 * ac[lag * 3]
        # 70–185 BPM 区间轻度加权，刻意不做 120 尖峰
        bpm = 60.0 * frame_rate / lag
        if 70.0 <= bpm <= 185.0:
            score *= 1.0
        else:
            score *= 0.85
        if score > best_score:
            best_score, best_lag = score, lag
    if best_lag <= 0:
        return None, 0.0
    peak = best_score / max(1e-9, abs(ac[0]))
    if peak < 0.30:
        return None, peak
    # 抛物线插值
    if 0 < best_lag < n - 1:
        y0, y1, y2 = ac[best_lag - 1], ac[best_lag], ac[best_lag + 1]
        denom = y0 - 2 * y1 + y2
        delta = 0.5 * (y0 - y2) / denom if abs(denom) > 1e-12 else 0.0
        lag = best_lag + max(-0.5, min(0.5, delta))
    else:
        lag = float(best_lag)
    bpm = 60.0 * frame_rate / lag
    if bpm < 40.0 or bpm > 210.0:
        return None, peak
    # 低频倍频折叠：<70 BPM 时乘 2 落回 70–185，和 DSP 内核同一口径
    # （Bit Bit Loop 实测 50.0，真实律动是 100 附近）。
    while bpm < 70.0 and bpm * 2.0 <= 185.0:
        bpm *= 2.0
    return float(bpm), float(min(1.0, max(0.0, peak)))



def read_wav(path: str) -> tuple[np.ndarray, int]:
    with wave.open(path, "rb") as w:
        if w.getnchannels() != 1 or w.getsampwidth() != 2:
            raise SystemExit("runner 只接受 16-bit 单声道 WAV（由主程序解码生成）")
        rate = w.getframerate()
        frames = w.readframes(w.getnframes())
    data = np.frombuffer(frames, dtype="<i2").astype(np.float32) / 32768.0
    return data, rate


def resample(x: np.ndarray, src: int, dst: int) -> np.ndarray:
    if src == dst:
        return x
    n = int(round(len(x) * dst / src))
    if n <= 0:
        return x
    idx = np.linspace(0, len(x) - 1, n, dtype=np.float64)
    return np.interp(idx, np.arange(len(x), dtype=np.float64), x).astype(np.float32)


def mel_filterbank(sr: int, n_fft: int, n_mels: int, fmin: float, fmax: float) -> np.ndarray:
    def hz_to_mel(f):
        return 2595.0 * math.log10(1.0 + f / 700.0)

    def mel_to_hz(m):
        return 700.0 * (10.0 ** (m / 2595.0) - 1.0)

    mels = np.linspace(hz_to_mel(fmin), hz_to_mel(fmax), n_mels + 2)
    hz = mel_to_hz(mels)
    bins = np.floor((n_fft + 1) * hz / sr).astype(int)
    fb = np.zeros((n_mels, n_fft // 2 + 1), dtype=np.float32)
    for i in range(n_mels):
        left, center, right = bins[i], bins[i + 1], bins[i + 2]
        if center == left:
            center += 1
        if right == center:
            right += 1
        for j in range(left, min(center, fb.shape[1])):
            fb[i, j] = (j - left) / (center - left)
        for j in range(center, min(right, fb.shape[1])):
            fb[i, j] = (right - j) / (right - center)
    return fb


def log_mel(x: np.ndarray, sr: int, mel: dict) -> np.ndarray:
    n_fft = int(mel.get("nFft", 1024))
    hop = int(mel.get("hop", 512))
    n_mels = int(mel.get("nMels", 128))
    fmin = float(mel.get("fmin", 30.0))
    fmax = float(mel.get("fmax", sr / 2))
    if len(x) < n_fft:
        x = np.pad(x, (0, n_fft - len(x)))
    frames = 1 + (len(x) - n_fft) // hop
    win = np.hanning(n_fft).astype(np.float32)
    fb = mel_filterbank(sr, n_fft, n_mels, fmin, fmax)
    spec = np.empty((frames, n_mels), dtype=np.float32)
    for i in range(frames):
        seg = x[i * hop : i * hop + n_fft] * win
        mag = np.abs(np.fft.rfft(seg)).astype(np.float32)
        spec[i] = fb @ (mag ** 2)
    spec = 10.0 * np.log10(spec + 1e-10)
    mode = str(mel.get("normalize", "perMel")).lower()
    if mode == "permel":
        spec = (spec - spec.mean(axis=0, keepdims=True)) / (spec.std(axis=0, keepdims=True) + 1e-5)
    elif mode == "global":
        spec = (spec - spec.mean()) / (spec.std() + 1e-5)
    return spec.astype(np.float32)


def softmax(v: np.ndarray) -> np.ndarray:
    v = v.astype(np.float64)
    v = v - v.max()
    e = np.exp(v)
    return e / e.sum()


def load_session(path: str):
    import onnxruntime as ort  # 延迟导入：没装也能给出可读错误

    so = ort.SessionOptions()
    so.intra_op_num_threads = max(1, min(4, os.cpu_count() or 1))
    return ort.InferenceSession(path, sess_options=so, providers=["CPUExecutionProvider"])


def run_basic_pitch(session, audio: np.ndarray, window: int) -> tuple[np.ndarray, np.ndarray, float]:
    """Basic Pitch（Apache-2.0）：波形进、音符/起音激活出。
    返回 (onset_env, chroma, frame_rate)。分块非重叠推理，块边界对 tempo 影响可忽略。
    """
    inp = session.get_inputs()[0]
    outs = session.get_outputs()
    pitch_outputs = [o for o in outs if o.shape and len(o.shape) == 3 and o.shape[-1] == 88]
    if len(pitch_outputs) < 2:
        raise RuntimeError("Basic Pitch 模型应有两个 88 列输出（音符 / 起音）")

    onset_env: list[float] = []
    chroma = np.zeros(12, dtype=np.float64)
    frames_per_chunk = 172
    starts = list(range(0, max(1, len(audio) - window + 1), window))
    for start in starts:
        chunk = audio[start : start + window]
        if chunk.size < window:
            chunk = np.pad(chunk, (0, window - chunk.size))
        fed = chunk.reshape(1, window, 1).astype(np.float32)
        raw = session.run([o.name for o in pitch_outputs], {inp.name: fed})
        arrays = [np.asarray(r).reshape(-1, arrays_last(r)) for r in raw]

        # 两个输出都是 [frames, 88]：时间导数大的那个是起音（onset），平滑的是音符（note）。
        deriv = [float(np.abs(np.diff(a, axis=0)).mean()) for a in arrays]
        note_idx = int(np.argmin(deriv))
        onset_idx = int(np.argmax(deriv))
        onset_env.extend(np.asarray(arrays[onset_idx]).sum(axis=1).tolist())
        note = np.asarray(arrays[note_idx])
        # chroma：88 个音高折到 12 音级。Basic Pitch 的音高轴是 A0–C8（88 键），
        # 因此第 0 个音高是 MIDI 21（A0），不是 C1=24；差 3 个半音会让整条调性偏移。
        weights = np.clip(note, 0, None).sum(axis=0)  # 每个音高在全曲的激活总量
        for pitch in range(88):
            chroma[(pitch + 21) % 12] += float(weights[pitch])

    frame_rate = frames_per_chunk * 22050.0 / window
    return np.asarray(onset_env, dtype=np.float64), chroma, frame_rate


def arrays_last(r: np.ndarray) -> int:
    return int(r.shape[-1])


def run_spec_backend(spec: dict, models_dir: str, audio: np.ndarray) -> dict:
    """按 spec.backend 分派；返回 bpm/key 及置信度（未定一律 None）。"""
    backend = str(spec.get("backend", "mel")).lower()
    result = {"bpm": None, "bpmConfidence": 0.0, "key": None, "keyConfidence": 0.0, "keyTop2": None}
    notes: list[str] = []

    if backend == "basic_pitch":
        model_path = os.path.join(models_dir, spec.get("model", "nmp.onnx"))
        session = load_session(model_path)
        onset_env, chroma, frame_rate = run_basic_pitch(session, audio, int(spec.get("window", 43844)))
        # 稳定性守卫：起音包络的变异系数太小说明根本没有拍点（稳态和弦实测 0.052，
        # 真实律动 0.115–0.192）。这一道补的是模型在持续音上凑出慢周期的假阳性。
        variability = float(onset_env.std() / (onset_env.mean() + 1e-9)) if onset_env.size else 0.0
        min_var = float(spec.get("minVariability", 0.08))
        if variability < min_var:
            result["bpm"] = None
            result["bpmConfidence"] = 0.0
            notes.append(f"起音包络变异系数 {variability:.3f} < {min_var}，判稳态/无拍点")
            peak = 0.0
        else:
            bpm, peak = tempo_from_envelope(onset_env, frame_rate)
            result["bpm"] = bpm
            result["bpmConfidence"] = round(peak, 3)
            if bpm is None:
                notes.append(f"起音包络自相关峰值 {peak:.2f} 未过 0.30，判 undetermined")
        key, top2, kconf = key_from_chroma(chroma)
        result["key"] = key
        result["keyTop2"] = top2
        result["keyConfidence"] = round(kconf, 3)
        notes.append(f"Basic Pitch 起音 {len(onset_env)} 帧 @ {frame_rate:.1f} fps；音符激活折 chroma（音高轴 A0–C8）")
        return {**result, "notes": notes}

    # 通用 mel 分类器（tempo / key 类别标签）
    mel = log_mel(audio, int(spec.get("sampleRate", 22050)), spec.get("mel", {}))
    tempo = spec.get("tempo")
    if tempo:
        session = load_session(os.path.join(models_dir, tempo["onnx"]))
        probs = run_model(session, tempo.get("chunkFrames", 187), mel)
        labels = np.asarray([float(v) for v in label_array(tempo, "tempo")])
        if probs.size != labels.size:
            notes.append(f"tempo 输出 {probs.size} 类与 labels {labels.size} 不符，跳过")
        else:
            top = int(probs.argmax())
            conf = float(probs[top])
            if conf >= float(tempo.get("minConfidence", 0.25)):
                result["bpm"] = round(float(labels[top]), 1)
                result["bpmConfidence"] = round(conf, 3)
            else:
                notes.append(f"tempo 峰值概率 {conf:.2f} 低于阈值，判 undetermined")
    else:
        notes.append("spec 未配置 tempo 模型")

    key_cfg = spec.get("key")
    if key_cfg:
        session = load_session(os.path.join(models_dir, key_cfg["onnx"]))
        probs = run_model(session, key_cfg.get("chunkFrames", 187), mel)
        labels = label_array(key_cfg, "key")
        if len(labels) >= 24 and probs.size == len(labels):
            top = int(probs.argmax())
            second = int(np.argsort(probs)[-2])
            result["key"] = labels[top]
            result["keyTop2"] = labels[second]
            result["keyConfidence"] = round(float(probs[top]), 3)
        elif probs.size == 12:
            chroma = probs / (probs.sum() + 1e-9)
            key, top2, kconf = key_from_chroma(chroma)
            result["key"] = key
            result["keyTop2"] = top2
            result["keyConfidence"] = round(kconf, 3)
            notes.append("模型只输出 12 音级，调性由权重表推出")
        else:
            notes.append(f"key 输出 {probs.size} 类与 labels {len(labels)} 不符，跳过")
    else:
        notes.append("spec 未配置 key 模型")
    return {**result, "notes": notes}


def run_model(session, spec_chunk_frames: int, mel_spec: np.ndarray) -> np.ndarray:
    """把 mel 切成定长块逐块推理，softmax 后取平均。"""
    chunk = int(spec_chunk_frames) if spec_chunk_frames else 187
    if mel_spec.shape[0] < chunk:
        mel_spec = np.pad(mel_spec, ((0, chunk - mel_spec.shape[0]), (0, 0)), mode="constant")
    probs = None
    count = 0
    for start in range(0, mel_spec.shape[0] - chunk + 1, chunk):
        block = mel_spec[start : start + chunk]
        fed = None
        for shape in ((1, 1, chunk, block.shape[1]), (1, chunk, block.shape[1]), (chunk, block.shape[1])):
            try:
                inp = session.get_inputs()[0]
                arr = block.reshape(shape).astype(np.float32)
                out = session.run(None, {inp.name: arr})[0]
                fed = np.asarray(out).reshape(-1)
                break
            except Exception:
                continue
        if fed is None:
            raise RuntimeError("模型输入形状不接受 (1,1,T,F)/(1,T,F)/(T,F) 三种排布，需按实际模型改 runner")
        p = softmax(fed)
        probs = p if probs is None else probs + p
        count += 1
    if probs is None:
        raise RuntimeError("音频太短，切不出一个模型输入块")
    return probs / count


def label_array(entry: dict, key: str) -> list:
    if entry.get("labels"):
        return entry["labels"]
    labels_file = entry.get("labelsFile")
    if labels_file:
        with open(labels_file, "r", encoding="utf-8") as f:
            return json.load(f)
    raise RuntimeError(f"spec 里 {key} 缺少 labels / labelsFile")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--audio", required=False)
    ap.add_argument("--config", required=False)
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()

    cfg_path = args.config or os.environ.get("BPM_MODEL_CONFIG")
    if not cfg_path or not os.path.exists(cfg_path):
        print(json.dumps({"ready": False, "detail": "找不到 model-engine.json"}))
        return 2
    with open(cfg_path, "r", encoding="utf-8") as f:
        cfg = json.load(f)
    models_dir = cfg.get("modelsDir") or ""
    spec_path = cfg.get("spec") or os.path.join(models_dir, "spec.json")

    if args.check:
        ok, detail = True, []
        if not os.path.isdir(models_dir):
            ok, detail = False, [f"模型目录不存在：{models_dir}"]
        else:
            if not os.path.exists(spec_path):
                ok = False
                detail.append(f"缺 spec.json：{spec_path}")
            else:
                with open(spec_path, "r", encoding="utf-8") as f:
                    spec = json.load(f)
                backend = str(spec.get("backend", "mel")).lower()
                if backend == "basic_pitch":
                    onnx_path = (
                        spec.get("model", "nmp.onnx")
                        if os.path.isabs(spec.get("model", ""))
                        else os.path.join(models_dir, spec.get("model", "nmp.onnx"))
                    )
                    if not os.path.exists(onnx_path):
                        ok = False
                        detail.append(f"缺权重：{onnx_path}")
                for role in ("tempo", "key"):
                    e = spec.get(role)
                    if not e:
                        continue
                    onnx_path = e.get("onnx") if os.path.isabs(e.get("onnx", "")) else os.path.join(models_dir, e.get("onnx", ""))
                    if not os.path.exists(onnx_path):
                        ok = False
                        detail.append(f"缺权重 {role}：{onnx_path}")
            try:
                import onnxruntime  # noqa: F401
            except Exception as e:
                ok = False
                detail.append(f"onnxruntime 不可用：{e}")
        print(json.dumps({"ready": ok, "detail": "；".join(detail) or "就绪"}))
        return 0 if ok else 3

    if not args.audio or not os.path.exists(args.audio):
        print(json.dumps({"error": "缺少 --audio 或文件不存在"}))
        return 4

    with open(spec_path, "r", encoding="utf-8") as f:
        spec = json.load(f)

    audio, sr = read_wav(args.audio)
    sr_target = int(spec.get("sampleRate", 22050))
    audio = resample(audio, sr, sr_target)

    result = {
        "bpm": None,
        "bpmConfidence": 0.0,
        "key": None,
        "keyConfidence": 0.0,
        "keyTop2": None,
        "model": spec.get("name", "spec"),
        "notes": [],
        "durationSec": round(len(audio) / sr_target, 2),
        "sampleRate": sr_target,
    }
    out = run_spec_backend(spec, models_dir, audio)
    result.update({k: v for k, v in out.items() if k != "notes"})
    result["notes"] = out.get("notes", [])

    print(json.dumps(result, ensure_ascii=False))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except SystemExit:
        raise
    except Exception as exc:  # 把异常压成一行诊断，交给主程序展示
        print(json.dumps({"error": str(exc)}, ensure_ascii=False))
        print(f"model_runner: {exc}", file=sys.stderr)
        sys.exit(5)
