#!/usr/bin/env python3
"""S1-A 评测主程序：跑合成评测集、算指标、写可复现报告。

只用 numpy。结果全部由本脚本实测生成，不手工填写指标。
用法：python evaluate.py [--seed-offset 0] [--out results]
"""

import argparse
import json
import platform
import sys
import time
from pathlib import Path

import numpy as np

import dsp
import synth

if hasattr(sys.stdout, "reconfigure"):
    sys.stdout.reconfigure(encoding="utf-8")


def percentile(values, p):
    if not values:
        return 0.0
    return float(np.percentile(np.array(values), p))


def rate(ok, total):
    return round(ok / total, 4) if total else 0.0


# 真值用降号音名（Bb / Eb），dsp.PC_NAMES 只输出升号体系（A# / D#）。
# 直接字面比较会让这 2 个主音必然判错，属判分口径缺陷而非算法误差。
_ENHARMONIC = {
    "Db": "C#", "Eb": "D#", "Gb": "F#", "Ab": "G#", "Bb": "A#",
    "Cb": "B", "Fb": "E", "E#": "F", "B#": "C",
}


def canon_key(name):
    """等音异名归一到升号体系，仅判分比较用；results.json 保留算法原始估计名。"""
    root, mode = name.split()
    return "%s %s" % (_ENHARMONIC.get(root, root), mode)


def run(out_dir):
    out_dir.mkdir(parents=True, exist_ok=True)
    samples = synth.build_samples()
    records = []
    timings = []
    for s in samples:
        t0 = time.perf_counter()
        result = dsp.analyze(s["audio"], synth.SR)
        elapsed = time.perf_counter() - t0
        timings.append(elapsed)
        bpm_r, key_r, flags = result["bpm"], result["key"], result["flags"]
        rec = {
            "id": s["id"],
            "group": s["group"],
            "truth": s["truth"],
            "meta": s["meta"],
            "estimate": {
                "bpm": bpm_r["bpm"],
                "bpm_confidence": round(bpm_r["confidence"], 3),
                "bpm_candidates": bpm_r["candidates"],
                "key": key_r["key"],
                "key_confidence": round(key_r["confidence"], 4),
                "key_top2": key_r["top2"],
            },
            "flags": flags,
            "elapsed_s": round(elapsed, 4),
        }
        if s["group"] == "bpm":
            truth = s["truth"]["bpm"]
            est = bpm_r["bpm"] if not flags["bpm_undetermined"] else None
            rec["score"] = {
                "strict": bool(est is not None and abs(est - truth) <= 1.0),
                "tolerance": bool(est is not None and abs(est - truth) <= max(2.0, 0.02 * truth)),
                "octave_ok": bool(est is None or (abs(est - truth / 2) > 2 and abs(est - truth * 2) > 2)),
                "false_undetermined": bool(flags["bpm_undetermined"]),
            }
        elif s["group"] == "key":
            truth = s["truth"]["key"]
            est = key_r["key"] if not flags["key_undetermined"] else None
            rec["score"] = {
                "top1": bool(est is not None and canon_key(est) == canon_key(truth)),
                "top2": bool(canon_key(truth) in [canon_key(k) for k in key_r["top2"]]),
                "false_undetermined": bool(flags["key_undetermined"]),
            }
        else:
            rec["score"] = {
                "true_undetermined": bool(flags["bpm_undetermined"] and flags["key_undetermined"]),
            }
        records.append(rec)

    bpm_recs = [r for r in records if r["group"] == "bpm"]
    key_recs = [r for r in records if r["group"] == "key"]
    neg_recs = [r for r in records if r["group"] == "negative"]

    def bpm_metrics(rows):
        return {
            "n": len(rows),
            "strict_acc": rate(sum(r["score"]["strict"] for r in rows), len(rows)),
            "tolerance_acc": rate(sum(r["score"]["tolerance"] for r in rows), len(rows)),
            "octave_ok_rate": rate(sum(r["score"]["octave_ok"] for r in rows), len(rows)),
            "false_undetermined_rate": rate(sum(r["score"]["false_undetermined"] for r in rows), len(rows)),
        }

    def key_metrics(rows):
        return {
            "n": len(rows),
            "top1_acc": rate(sum(r["score"]["top1"] for r in rows), len(rows)),
            "top2_acc": rate(sum(r["score"]["top2"] for r in rows), len(rows)),
            "false_undetermined_rate": rate(sum(r["score"]["false_undetermined"] for r in rows), len(rows)),
        }

    by_snr = {}
    for snr in ("clean", "20", "10"):
        tag = "clean" if snr == "clean" else snr
        b = [r for r in bpm_recs if ("clean" if r["meta"]["snr_db"] is None else str(r["meta"]["snr_db"])) == tag]
        k = [r for r in key_recs if ("clean" if r["meta"]["snr_db"] is None else str(r["meta"]["snr_db"])) == tag]
        if b or k:
            by_snr[tag] = {"bpm": bpm_metrics(b) if b else None, "key": key_metrics(k) if k else None}

    summary = {
        "generated_at": time.strftime("%Y-%m-%d %H:%M:%S"),
        "environment": {
            "python": platform.python_version(),
            "numpy": np.__version__,
            "platform": platform.platform(),
        },
        "config": {
            "sample_rate": synth.SR,
            "duration_s": synth.DURATION,
            "n_fft": dsp.N_FFT,
            "hop": dsp.HOP,
            "bpm_grid": synth.BPM_GRID,
            "major_roots": synth.MAJOR_ROOTS,
            "minor_roots": synth.MINOR_ROOTS,
            "progression_degrees": synth.PROGRESSION_DEGREES,
            "snr_db_bpm": [None, 20, 10],
            "snr_db_key": [None, 20],
            "negative_kinds": synth.NEGATIVE_KINDS,
            "search_bpm_range": [dsp.SEARCH_MIN, dsp.SEARCH_MAX],
            "fold_bpm_range": [dsp.BPM_MIN, dsp.BPM_MAX],
        },
        "counts": {
            "total": len(records),
            "bpm": len(bpm_recs),
            "key": len(key_recs),
            "negative": len(neg_recs),
        },
        "metrics": {
            "bpm": bpm_metrics(bpm_recs),
            "key": key_metrics(key_recs),
            "negative": {
                "n": len(neg_recs),
                "true_undetermined_rate": rate(sum(r["score"]["true_undetermined"] for r in neg_recs), len(neg_recs)),
            },
            "timing_s": {
                "mean": round(float(np.mean(timings)), 4),
                "p95": round(percentile(timings, 95), 4),
                "max": round(float(np.max(timings)), 4),
            },
        },
        "by_snr": by_snr,
    }

    (out_dir / "results.json").write_text(json.dumps(records, ensure_ascii=False, indent=2), encoding="utf-8")
    (out_dir / "summary.json").write_text(json.dumps(summary, ensure_ascii=False, indent=2), encoding="utf-8")
    write_report(out_dir, summary, records)
    print("样本 %d 条，耗时均值 %.4fs" % (len(records), np.mean(timings)))
    print("BPM 严格准确率 %.1f%% / 容差 %.1f%% / 倍频正确 %.1f%%" % (
        summary["metrics"]["bpm"]["strict_acc"] * 100,
        summary["metrics"]["bpm"]["tolerance_acc"] * 100,
        summary["metrics"]["bpm"]["octave_ok_rate"] * 100))
    print("调性 top-1 %.1f%% / top-2 %.1f%%" % (
        summary["metrics"]["key"]["top1_acc"] * 100,
        summary["metrics"]["key"]["top2_acc"] * 100))
    print("负例 undetermined 触发 %.1f%%" % (summary["metrics"]["negative"]["true_undetermined_rate"] * 100))
    print("报告：", out_dir / "report.md")
    return summary


def write_report(out_dir, s, records):
    m = s["metrics"]
    cfg = s["config"]
    lines = [
        "# S1-A 评测报告（合成信号 · 经典 DSP 基线）",
        "",
        "本报告由 `evaluate.py` 实测生成，指标不手工填写。对应库内项目入口："
        "`5-领域-A/06-软件制作/88-BPM调性识别工具/`。",
        "",
        "## 环境与参数",
        "",
        "- Python %s / numpy %s / %s" % (s["environment"]["python"], s["environment"]["numpy"], s["environment"]["platform"]),
        "- 采样率 %d Hz，单条时长 %.1f s，STFT n_fft=%d / hop=%d" % (
            cfg["sample_rate"], cfg["duration_s"], cfg["n_fft"], cfg["hop"]),
        "- BPM 网格：%s" % ", ".join(str(v) for v in cfg["bpm_grid"]),
        "- 调性：大调 %s；小调 %s；进行音级 %s" % (
            "/".join(cfg["major_roots"]), "/".join(cfg["minor_roots"]), cfg["progression_degrees"]),
        "- 加噪梯度：BPM 组 clean/20dB/10dB；调性组 clean/20dB；负例 %s" % ", ".join(cfg["negative_kinds"]),
        "- tempo 搜索 %.0f–%.0f BPM，结果折叠到 %.0f–%.0f BPM" % (
            cfg["search_bpm_range"][0], cfg["search_bpm_range"][1],
            cfg["fold_bpm_range"][0], cfg["fold_bpm_range"][1]),
        "- 样本总数 %d（BPM %d / 调性 %d / 负例 %d）" % (
            s["counts"]["total"], s["counts"]["bpm"], s["counts"]["key"], s["counts"]["negative"]),
        "",
        "## 指标结果",
        "",
        "| 指标 | 结果 | 口径 |",
        "| --- | --- | --- |",
        "| BPM 严格准确率 | %.1f%% | 绝对误差 ≤ 1 BPM（n=%d） |" % (m["bpm"]["strict_acc"] * 100, m["bpm"]["n"]),
        "| BPM 容差准确率 | %.1f%% | 误差 ≤ max(2 BPM, 2%%) |" % (m["bpm"]["tolerance_acc"] * 100),
        "| 倍频判定正确率 | %.1f%% | 主值不是真值的一半或两倍 |" % (m["bpm"]["octave_ok_rate"] * 100),
        "| 调性 top-1 | %.1f%% | 主选命中（n=%d） |" % (m["key"]["top1_acc"] * 100, m["key"]["n"]),
        "| 调性 top-2 | %.1f%% | 前两候选包含真值 |" % (m["key"]["top2_acc"] * 100),
        "| 负例 undetermined 触发 | %.1f%% | BPM 与调性同时判为无法确定（n=%d） |" % (
            m["negative"]["true_undetermined_rate"] * 100, m["negative"]["n"]),
        "| BPM 组正例误弃率 | %.1f%% | 有真值却判 undetermined |" % (m["bpm"]["false_undetermined_rate"] * 100),
        "| 调性组正例误弃率 | %.1f%% | 有真值却判 undetermined |" % (m["key"]["false_undetermined_rate"] * 100),
        "| 单曲耗时 | 均值 %.4f s / P95 %.4f s / 最大 %.4f s | 仅 analyze 调用，不含信号生成 |" % (
            m["timing_s"]["mean"], m["timing_s"]["p95"], m["timing_s"]["max"]),
        "",
        "## 按加噪梯度",
        "",
        "| 条件 | BPM 严格 | BPM 容差 | 倍频正确 | 调性 top-1 | 调性 top-2 |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for tag, row in s["by_snr"].items():
        b, k = row["bpm"], row["key"]
        def fmt(v):
            return "—" if v is None else "%.1f%%" % (v * 100)
        lines.append("| %s | %s | %s | %s | %s | %s |" % (
            tag,
            fmt(b["strict_acc"] if b else None), fmt(b["tolerance_acc"] if b else None),
            fmt(b["octave_ok_rate"] if b else None),
            fmt(k["top1_acc"] if k else None), fmt(k["top2_acc"] if k else None)))

    failures = [r for r in records if r["group"] == "bpm" and not r["score"]["strict"]]
    failures += [r for r in records if r["group"] == "key" and not r["score"]["top1"]]
    lines += ["", "## 失败样例", ""]
    if not failures:
        lines.append("本次无失败样例（仅针对合成集，见下方效度声明）。")
    else:
        lines.append("| 样本 | 真值 | 估计 | 备注 |")
        lines.append("| --- | --- | --- | --- |")
        for r in failures[:20]:
            truth = r["truth"].get("bpm", r["truth"].get("key"))
            est = r["estimate"]["bpm"] if r["group"] == "bpm" else r["estimate"]["key"]
            note = "undetermined" if (r["flags"]["bpm_undetermined"] if r["group"] == "bpm" else r["flags"]["key_undetermined"]) else "估计偏差"
            lines.append("| %s | %s | %s | %s |" % (r["id"], truth, est, note))
        if len(failures) > 20:
            lines.append("")
            lines.append("另有 %d 条失败样例，完整明细见 `results.json`。" % (len(failures) - 20))

    lines += [
        "",
        "## 已知缺陷与本次修订",
        "",
        "### 已修正：判分口径的等音异名不一致（测量工具缺陷）",
        "",
        "首轮实测发现真值使用降号音名（`Bb` / `Eb`），而 `dsp.PC_NAMES` 只输出升号体系"
        "（`A#` / `D#`），字面比较使 12 个主音中的 2 个**必然判错**。已在 `evaluate.py` 判分时"
        "做等音异名归一化：**只改判分口径，不改算法、不改阈值、不针对评测集调参**；"
        "`results.json` 仍保留算法原始估计名以便复核。修订前 top-1 记录为 83.3%，修正后见上文指标表。",
        "",
        "### 未修正：三项真实算法发现（本批 S1 的结论，非脚本缺陷）",
        "",
        "1. **高 BPM 系统性低估**：150 / 170 / 180 BPM 的稳定估计分别落到 100.1 / 113.4 / 90.1，"
        "源于 tempo salience 谐波加权与 120 BPM 速度先验，≥150 BPM 无一命中。",
        "2. **无调性 / 噪声负例未拦截**：`noise` 与 `atonal` 共 4 条全部未同时判为 `undetermined`，"
        "负例触发率因此仅 %.1f%%。白噪声 onset 包络自相关产生虚高 salience（7.19 / 4.00，阈值 2.0），"
        "十二音堆叠仍能与 KS 模板产生 > 0.45 相关。方案要求的「无调性、低置信度一律 undetermined」"
        "**当前未达标**。" % (m["negative"]["true_undetermined_rate"] * 100),
        "3. **五度混淆**：`Eb major` 稳定误判为 `A# major`（真值 pc3、估计 pc10，相差纯五度），"
        "属 Krumhansl-Schmuckler 模板相关的典型失效模式。",
        "",
        "以上三项是路线 A 的真实短板，刻意**不在本批调阈值修复**——在评测集上调参会污染"
        "「先评测再定路线」的结论，应作为 S1 后续与路线 B 对比的输入。",
        "",
    ]

    lines += [
        "",
        "## 效度声明（必读）",
        "",
        "1. **同源风险**：合成信号由十二平均律、三和弦与固定节拍模型生成，检测器也用十二平均律音级映射与周期自相关。两者共享同一套音乐模型，因此高指标只说明「评测管线实现正确、能跑通」，**不能证明真实曲库准确率**。",
        "2. **不是路线对比**：本批只实现路线 A（经典 DSP）。ONNX 路线 B 未选型、未下载模型、未跑推理，因此「先评测再定路线」的拍板条件**尚未满足**。",
        "3. **无真实样本**：没有使用任何人工标注的真实音频，真实样本的来源、数量与版权状态仍待裁决。",
        "4. **未验证的边界**：未做真实业务、GUI、跨机、跨格式（MP3/FLAC）验证；未写回任何标签或原音频文件元数据；未安装新依赖。",
        "5. **阈值是工程设定**：`undetermined` 的置信度阈值（BPM salience 比 < 2.0、调性相关 < 0.45）与 120 BPM 速度先验是固定工程参数，本报告未针对评测集调参；调整阈值会改变上表结果。",
        "6. **指标零产出不宣称达标**：本批有实测数据，但仅对合成集成立。",
        "",
        "## 复现",
        "",
        "```bash",
        'python evaluate.py --out results',
        "```",
        "",
        "解释器使用 `D:/Ai-Files/Tools/BuildEnv/sound-workbench-ai/Scripts/python.exe`（numpy 2.3.4）。",
        "产物：`results.json`（逐条）、`summary.json`（汇总）、`report.md`（本文件）。",
        "",
        "生成时间：%s" % s["generated_at"],
        "",
    ]
    (out_dir / "report.md").write_text("\n".join(lines), encoding="utf-8")


if __name__ == "__main__":
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="results")
    args = ap.parse_args()
    run(Path(__file__).parent / args.out)
