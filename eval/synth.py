#!/usr/bin/env python3
"""S1-A 合成评测信号生成器。

只用 numpy。生成已知 BPM 的点击音轨、已知调性的三和弦进行，以及四类负例。
本模块的生成模型与分析模型同属「十二平均律 + 调性音乐」框架，结果只能证明
评测管线实现正确，不能证明真实曲库准确率。详见 results/report.md。
"""

import numpy as np

SR = 22050
DURATION = 20.0

BPM_GRID = [60, 75, 90, 100, 110, 120, 128, 140, 150, 170, 180]
MAJOR_ROOTS = ["C", "G", "D", "F", "Bb", "Eb"]
MINOR_ROOTS = ["A", "E", "B", "D", "G", "C"]
NEGATIVE_KINDS = ["silence", "noise", "short", "atonal"]

NOTE_TO_PC = {
    "C": 0, "C#": 1, "Db": 1, "D": 2, "D#": 3, "Eb": 3,
    "E": 4, "F": 5, "F#": 6, "Gb": 6, "G": 7, "G#": 8,
    "Ab": 8, "A": 9, "A#": 10, "Bb": 10, "B": 11,
}
MAJOR_SCALE = [0, 2, 4, 5, 7, 9, 11]
MINOR_SCALE = [0, 2, 3, 5, 7, 8, 10]
PROGRESSION_DEGREES = [0, 3, 4, 0]


def _rng(seed):
    return np.random.default_rng(seed)


def _add_noise(x, rng, snr_db):
    if snr_db is None:
        return x
    p = float(np.mean(x ** 2))
    if p <= 0:
        return x
    noise = rng.normal(0, 1, len(x)).astype(np.float32)
    noise *= np.sqrt(p / (float(np.mean(noise ** 2)) + 1e-12) / (10 ** (snr_db / 10)))
    return np.clip(x + noise, -1, 1).astype(np.float32)


def _burst(rng, length):
    t = np.arange(length) / SR
    env = np.exp(-t / 0.012)
    noise = rng.normal(0, 1, length)
    tone = np.sin(2 * np.pi * 70 * t)
    return (0.7 * noise * env + 0.5 * tone * env).astype(np.float32)


def make_click_track(bpm, duration=DURATION, seed=0, snr_db=None):
    """已知 BPM 的点击音轨，每四拍一次强拍重音（刻意的倍频干扰）。"""
    rng = _rng(seed)
    n = int(duration * SR)
    x = np.zeros(n, dtype=np.float32)
    burst_len = int(0.05 * SR)
    b = _burst(rng, burst_len)
    period = 60.0 / bpm
    t, k = 0.0, 0
    while t + burst_len / SR < duration:
        s = int(t * SR)
        x[s:s + burst_len] += b * (1.0 if k % 4 == 0 else 0.7)
        t += period
        k += 1
    x += (rng.normal(0, 1, n) * 0.002).astype(np.float32)
    return _add_noise(x, rng, snr_db)


def _note_freq(pc, octave):
    midi = 12 * (octave + 1) + pc
    return 440.0 * 2 ** ((midi - 69) / 12)


def _chord(rng, pcs, duration, amp):
    n = int(duration * SR)
    t = np.arange(n) / SR
    y = np.zeros(n, dtype=np.float64)
    for pc in pcs:
        f = _note_freq(pc, 4)
        y += (np.sin(2 * np.pi * f * t)
              + 0.4 * np.sin(2 * np.pi * 2 * f * t)
              + 0.15 * np.sin(2 * np.pi * 3 * f * t))
    env = np.minimum(1.0, t / 0.02) * np.exp(-t / (duration * 0.8))
    return (amp * y * env).astype(np.float32)


def make_chord_progression(root, mode, duration=DURATION, seed=0, snr_db=None):
    """已知主音与调式的 I-IV-V-I 进行，按调内音级构建三和弦。"""
    rng = _rng(seed)
    n = int(duration * SR)
    x = np.zeros(n, dtype=np.float32)
    root_pc = NOTE_TO_PC[root]
    scale = MAJOR_SCALE if mode == "major" else MINOR_SCALE
    chord_len = duration / len(PROGRESSION_DEGREES)
    for i, degree in enumerate(PROGRESSION_DEGREES):
        pcs = [root_pc + scale[(degree + off) % 7] for off in (0, 2, 4)]
        pcs = [p % 12 for p in pcs]
        seg = _chord(rng, pcs, chord_len, amp=0.22)
        s = int(i * chord_len * SR)
        x[s:s + len(seg)] += seg
    x += (rng.normal(0, 1, n) * 0.0005).astype(np.float32)
    return _add_noise(x, rng, snr_db)


def make_negative(kind, seed=0, duration=DURATION):
    """四类应判为 undetermined 的负例。"""
    rng = _rng(seed)
    if kind == "silence":
        return np.zeros(int(duration * SR), dtype=np.float32)
    if kind == "noise":
        return (rng.normal(0, 1, int(duration * SR)) * 0.05).astype(np.float32)
    if kind == "short":
        return make_click_track(120, duration=0.5, seed=seed)
    if kind == "atonal":
        n = int(duration * SR)
        t = np.arange(n) / SR
        y = np.zeros(n, dtype=np.float64)
        for pc in range(12):
            y += np.sin(2 * np.pi * _note_freq(pc, 4) * t)
            y += 0.3 * np.sin(2 * np.pi * _note_freq((pc + 1) % 12, 5) * t)
        return (0.02 * y).astype(np.float32)
    raise ValueError(kind)


def label_of(root, mode):
    return root + (" major" if mode == "major" else " minor")


def build_samples():
    """返回样本列表，每条含 id / group / truth / audio / meta。"""
    samples = []
    for bpm in BPM_GRID:
        for snr in (None, 20, 10):
            seed = 1000 + bpm + (0 if snr is None else int(snr))
            samples.append({
                "id": "bpm-%d-snr-%s" % (bpm, "clean" if snr is None else str(snr)),
                "group": "bpm",
                "truth": {"bpm": bpm},
                "meta": {"snr_db": snr, "seed": seed, "duration": DURATION},
                "audio": make_click_track(bpm, seed=seed, snr_db=snr),
            })
    for mode, roots in (("major", MAJOR_ROOTS), ("minor", MINOR_ROOTS)):
        for root in roots:
            for snr in (None, 20):
                seed = 2000 + NOTE_TO_PC[root] + (0 if mode == "major" else 12) + (0 if snr is None else 40)
                samples.append({
                    "id": "key-%s-%s-snr-%s" % (root.replace("#", "s"), mode, "clean" if snr is None else str(snr)),
                    "group": "key",
                    "truth": {"key": label_of(root, mode)},
                    "meta": {"snr_db": snr, "seed": seed, "duration": DURATION},
                    "audio": make_chord_progression(root, mode, seed=seed, snr_db=snr),
                })
    for kind in NEGATIVE_KINDS:
        for seed in (0, 1):
            samples.append({
                "id": "neg-%s-%d" % (kind, seed),
                "group": "negative",
                "truth": {"undetermined": True},
                "meta": {"kind": kind, "seed": seed},
                "audio": make_negative(kind, seed=seed),
            })
    return samples
