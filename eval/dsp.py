#!/usr/bin/env python3
"""S1-A 经典 DSP 基线：纯 numpy 实现，不用 scipy / librosa / aubio。

BPM：STFT 谱通量 onset 包络 → 自相关 → 谐波加权 tempo salience → 抛物线插值 → 倍频折叠。
调性：平均谱映射到 12 音级 → chroma → 与 Krumhansl-Schmuckler 大/小调模板做相关。
"""

import numpy as np

N_FFT = 2048
HOP = 256
BPM_MIN, BPM_MAX = 60.0, 180.0
SEARCH_MIN, SEARCH_MAX = 50.0, 200.0
KS_MAJOR = np.array([6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88])
KS_MINOR = np.array([6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17])
PC_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]


def stft_mag(x, sr, n_fft=N_FFT, hop=HOP):
    if len(x) < n_fft:
        x = np.pad(x, (0, n_fft - len(x)))
    n_frames = 1 + (len(x) - n_fft) // hop
    idx = np.arange(n_fft)[None, :] + hop * np.arange(n_frames)[:, None]
    win = np.hanning(n_fft).astype(np.float32)
    frames = x[idx] * win
    return np.abs(np.fft.rfft(frames, axis=1)).astype(np.float32)


def onset_envelope(mag):
    flux = np.maximum(np.diff(mag, axis=0), 0).sum(axis=1)
    flux = np.concatenate([flux[:1], flux])
    baseline = np.convolve(flux, np.ones(9) / 9, mode="same")
    env = np.maximum(flux - baseline, 0)
    return np.convolve(env, np.ones(3) / 3, mode="same").astype(np.float64)


def _parabolic(y0, y1, y2):
    denom = y0 - 2 * y1 + y2
    if abs(denom) < 1e-12:
        return 0.0
    return float(np.clip(0.5 * (y0 - y2) / denom, -0.5, 0.5))


def estimate_bpm(env, sr, hop=HOP):
    e = env - env.mean()
    ac = np.correlate(e, e, mode="full")[len(e) - 1:]
    if ac[0] <= 0:
        return {"bpm": None, "confidence": 0.0, "candidates": [], "peaks": []}
    ac = ac / ac[0]
    lag_lo = max(3, int(60 * sr / (hop * SEARCH_MAX)))
    lag_hi = min(len(ac) - 1, int(60 * sr / (hop * SEARCH_MIN)) + 1)
    if lag_hi - lag_lo < 4:
        return {"bpm": None, "confidence": 0.0, "candidates": [], "peaks": []}

    def ac_at(c):
        c = int(round(c))
        return float(ac[c]) if 0 <= c < len(ac) else 0.0

    salience = np.zeros(lag_hi + 1, dtype=np.float64)
    for c in range(lag_lo, lag_hi + 1):
        salience[c] = ac_at(c) + 0.3 * ac_at(2 * c) + 0.15 * ac_at(3 * c)

    peaks = [c for c in range(lag_lo + 1, lag_hi)
             if salience[c] > salience[c - 1] and salience[c] >= salience[c + 1]]
    peaks.sort(key=lambda c: salience[c], reverse=True)
    peaks = peaks[:6]

    folded = {}
    for c in peaks:
        delta = _parabolic(salience[c - 1], salience[c], salience[c + 1])
        lag = max(1.0, c + delta)
        raw = 60.0 * sr / (hop * lag)
        bpm = raw
        while bpm < BPM_MIN:
            bpm *= 2
        while bpm > BPM_MAX:
            bpm /= 2
        key = round(bpm, 1)
        score = float(salience[c])
        if key not in folded or score > folded[key]["salience"]:
            folded[key] = {"bpm": key, "raw": round(raw, 2), "salience": score}

    candidates = sorted(folded.values(), key=lambda d: d["salience"], reverse=True)
    if not candidates:
        return {"bpm": None, "confidence": 0.0, "candidates": [], "peaks": peaks}

    scores = np.array([d["salience"] for d in candidates])
    priors = np.exp(-0.5 * (np.log2(np.array([d["bpm"] for d in candidates]) / 120.0) / 0.6) ** 2)
    weighted = scores * priors
    best = int(np.argmax(weighted))
    mean_sal = float(np.mean(np.maximum(salience[lag_lo:lag_hi + 1], 0))) + 1e-9
    confidence = float(candidates[best]["salience"] / mean_sal)
    return {
        "bpm": candidates[best]["bpm"],
        "confidence": confidence,
        "candidates": candidates[:3],
        "peaks": peaks,
    }


def chroma_vector(mag, sr, n_fft=N_FFT):
    freqs = np.fft.rfftfreq(n_fft, 1.0 / sr)
    band = (freqs >= 80) & (freqs <= 2000)
    f = freqs[band]
    m = mag[:, band].mean(axis=0)
    midi = 69 + 12 * np.log2(np.maximum(f, 1e-6) / 440.0)
    pc = np.round(midi).astype(np.int64) % 12
    c = np.bincount(pc, weights=m, minlength=12).astype(np.float64)
    return c / (c.sum() + 1e-12)


def build_profiles():
    profiles = {}
    for root in range(12):
        for mode, base in (("major", KS_MAJOR), ("minor", KS_MINOR)):
            profiles["%s %s" % (PC_NAMES[root], mode)] = np.roll(base, root)
    return profiles


PROFILES = build_profiles()


def estimate_key(chroma):
    c = chroma - chroma.mean()
    norm_c = np.linalg.norm(c) + 1e-12
    scored = []
    for name, p in PROFILES.items():
        q = p - p.mean()
        r = float(np.dot(c, q) / (norm_c * (np.linalg.norm(q) + 1e-12)))
        scored.append((name, r))
    scored.sort(key=lambda t: t[1], reverse=True)
    return {
        "key": scored[0][0],
        "confidence": scored[0][1],
        "top2": [s[0] for s in scored[:2]],
        "top2_scores": [round(s[1], 4) for s in scored[:2]],
    }


def undetermined_flags(x, sr, bpm_result, key_result):
    duration = len(x) / sr
    rms = float(np.sqrt(np.mean(x ** 2))) if len(x) else 0.0
    silent = rms < 1e-4
    too_short = duration < 2.0
    return {
        "duration": round(duration, 3),
        "rms": round(rms, 6),
        "bpm_undetermined": bool(silent or too_short or bpm_result["confidence"] < 2.0),
        "key_undetermined": bool(silent or too_short or key_result["confidence"] < 0.45),
    }


def analyze(x, sr):
    mag = stft_mag(x, sr)
    env = onset_envelope(mag)
    bpm_result = estimate_bpm(env, sr)
    key_result = estimate_key(chroma_vector(mag, sr))
    flags = undetermined_flags(x, sr, bpm_result, key_result)
    return {"bpm": bpm_result, "key": key_result, "flags": flags}
