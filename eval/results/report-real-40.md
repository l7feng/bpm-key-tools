# 真实素材评测报告（引擎：本地 DSP）

由 `cargo test --offline eval_real_manifest -- --ignored --nocapture` 生成，指标不手工填写。
清单 `eval/real-manifest.json`：`bpm` 为数值＝有真值；`bpm: null`＝应判 undetermined 的负例；不带 `bpm`＝尚无标注，只列观测值。

## 指标

| 指标 | 结果 | 口径 |
| --- | --- | --- |
| BPM 严格 | — | 绝对误差 ≤ 1 BPM（n=0） |
| BPM 容差 | — | 误差 ≤ max(2 BPM, 2%) |
| BPM 误弃 | 0 | 有真值却判 undetermined |
| 倍频错误 | 0 | 输出约为真值 2 倍或 0.5 倍 |
| 负例通过 | — | 应判 undetermined 且确实没给数（n=0） |
| 调性 top-1 | — | 主选命中（n=0） |
| 单条耗时 | 中位 4770 ms / 最大 22668 ms | 11,025 Hz 抽取后计算 |

读文件错误：0。

## 逐条

| id | 类型 | 真值 BPM | 实测 BPM | BPM 置信 | 调性 | 调性置信 | 耗时 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| loop-01 | loop | — | undetermined | 0.28 | B minor | 0.00 | 4479 ms |
| loop-02 | loop | — | undetermined | 0.23 | C major | 0.01 | 6011 ms |
| loop-03 | loop | — | undetermined | 0.18 | F major | 0.02 | 3175 ms |
| loop-04 | loop | — | undetermined | 0.29 | F major | 0.00 | 4402 ms |
| loop-05 | loop | — | 86.8 | 0.31 | G major | 0.00 | 2607 ms |
| loop-06 | loop | — | undetermined | 0.07 | A minor | 0.00 | 4052 ms |
| loop-07 | loop | — | 144.1 | 0.38 | F major | 0.01 | 3591 ms |
| loop-08 | loop | — | 140.7 | 0.59 | C major | 0.00 | 3903 ms |
| loop-09 | loop | — | 100.1 | 0.54 | C major | 0.01 | 3868 ms |
| loop-10 | loop | — | 100.1 | 0.41 | C major | 0.01 | 5891 ms |
| loop-11 | loop | — | 66.1 | 0.52 | C major | 0.01 | 4436 ms |
| loop-12 | loop | — | 86.8 | 0.50 | F major | 0.01 | 2802 ms |
| loop-13 | loop | — | undetermined | 0.07 | C major | 0.01 | 4168 ms |
| score-14 | score | — | undetermined | 0.11 | E major | 0.01 | 5285 ms |
| score-15 | score | — | undetermined | 0.14 | C# major | 0.00 | 2365 ms |
| score-16 | score | — | undetermined | 0.08 | F major | 0.01 | 7078 ms |
| score-17 | score | — | undetermined | 0.07 | C minor | 0.00 | 6628 ms |
| score-18 | score | — | 90.1 | 0.39 | F major | 0.01 | 9430 ms |
| score-19 | score | — | undetermined | 0.12 | A minor | 0.02 | 2813 ms |
| score-20 | score | — | 100.1 | 0.67 | F major | 0.01 | 6335 ms |
| score-21 | score | — | undetermined | 0.13 | C major | 0.00 | 6946 ms |
| score-22 | score | — | undetermined | 0.11 | F# minor | 0.02 | 8004 ms |
| score-23 | score | — | undetermined | 0.07 | F# minor | 0.01 | 4197 ms |
| score-24 | score | — | undetermined | 0.07 | C major | 0.01 | 5291 ms |
| score-25 | score | — | undetermined | 0.08 | B minor | 0.00 | 6377 ms |
| score-26 | score | — | undetermined | 0.07 | C minor | 0.00 | 3358 ms |
| orchestral-27 | orchestral | — | undetermined | 0.08 | F# minor | 0.01 | 22668 ms |
| orchestral-28 | orchestral | — | undetermined | 0.06 | A minor | 0.02 | 5815 ms |
| orchestral-29 | orchestral | — | 135.5 | 0.40 | C minor | 0.01 | 6634 ms |
| orchestral-30 | orchestral | — | undetermined | 0.14 | E minor | 0.01 | 8296 ms |
| orchestral-31 | orchestral | — | 160.2 | 0.31 | C minor | 0.00 | 5145 ms |
| orchestral-32 | orchestral | — | undetermined | 0.09 | C minor | 0.01 | 9820 ms |
| orchestral-33 | orchestral | — | undetermined | 0.13 | B minor | 0.01 | 3422 ms |
| orchestral-34 | orchestral | — | undetermined | 0.09 | F major | 0.01 | 3895 ms |
| orchestral-35 | orchestral | — | undetermined | 0.06 | E minor | 0.00 | 4770 ms |
| orchestral-36 | orchestral | — | undetermined | 0.08 | F major | 0.00 | 6567 ms |
| orchestral-37 | orchestral | — | undetermined | 0.21 | E major | 0.01 | 7875 ms |
| orchestral-38 | orchestral | — | undetermined | 0.28 | D minor | 0.02 | 2913 ms |
| orchestral-39 | orchestral | — | undetermined | 0.10 | G# minor | 0.01 | 2655 ms |
| orchestral-40 | orchestral | — | undetermined | 0.28 | C minor | 0.02 | 3735 ms |

## 负例按类型

| 类型 | 条数 | 判 undetermined |
| --- | --- | --- |
