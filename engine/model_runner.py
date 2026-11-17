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
    mel = log_mel(audio, sr_target, spec.get("mel", {}))

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

    tempo = spec.get("tempo")
    if tempo:
        session = load_session(os.path.join(models_dir, tempo["onnx"]))
        probs = run_model(session, tempo.get("chunkFrames", 187), mel)
        labels = np.asarray([float(v) for v in label_array(tempo, "tempo")])
        if probs.size != labels.size:
            result["notes"].append(f"tempo 输出 {probs.size} 类与 labels {labels.size} 不符，跳过")
        else:
            top = int(probs.argmax())
            bpm = float(labels[top])
            conf = float(probs[top])
            # 用邻近类的概率质量做一个粗略的稳定性判据：峰太散就判 undetermined。
            if conf >= float(tempo.get("minConfidence", 0.25)):
                result["bpm"] = round(bpm, 1)
                result["bpmConfidence"] = round(conf, 3)
            else:
                result["notes"].append(f"tempo 峰值概率 {conf:.2f} 低于阈值，判 undetermined")
    else:
        result["notes"].append("spec 未配置 tempo 模型")

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
            result["key"] = KEY_NAMES[int(chroma.argmax())]
            result["keyConfidence"] = round(float(chroma.max()), 3)
            result["notes"].append("模型只输出 12 音级（未分大小调）")
        else:
            result["notes"].append(f"key 输出 {probs.size} 类与 labels {len(labels)} 不符，跳过")
    else:
        result["notes"].append("spec 未配置 key 模型")

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
