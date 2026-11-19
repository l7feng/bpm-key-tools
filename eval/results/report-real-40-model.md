# 真实素材评测报告（引擎：神经网络模型）

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
| 单条耗时 | 中位 5324 ms / 最大 31201 ms | 11,025 Hz 抽取后计算 |

读文件错误：0。

## 逐条

| id | 类型 | 真值 BPM | 实测 BPM | BPM 置信 | 调性 | 调性置信 | 耗时 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| loop-01 | loop | — | 169.8 | 0.80 | E minor | 0.00 | 5324 ms |
| loop-02 | loop | — | 105.0 | 0.77 | A minor | 0.00 | 6599 ms |
| loop-03 | loop | — | 125.4 | 0.62 | A minor | 0.00 | 3573 ms |
| loop-04 | loop | — | 70.0 | 0.95 | G minor | 0.00 | 4712 ms |
| loop-05 | loop | — | 131.3 | 0.63 | G minor | 0.00 | 2850 ms |
| loop-06 | loop | — | 72.5 | 0.92 | E minor | 0.00 | 4542 ms |
| loop-07 | loop | — | 143.8 | 0.51 | A minor | 0.00 | 4128 ms |
| loop-08 | loop | — | 140.0 | 1.00 | A minor | 0.00 | 4649 ms |
| loop-09 | loop | — | 99.9 | 0.92 | A minor | 0.00 | 4687 ms |
| loop-10 | loop | — | 99.2 | 0.76 | A minor | 0.00 | 6890 ms |
| loop-11 | loop | — | 132.2 | 0.88 | G minor | 0.00 | 4725 ms |
| loop-12 | loop | — | 86.2 | 0.55 | E minor | 0.00 | 3065 ms |
| loop-13 | loop | — | 131.4 | 0.41 | C minor | 0.00 | 4600 ms |
| score-14 | score | — | 120.7 | 0.78 | E minor | 0.00 | 5933 ms |
| score-15 | score | — | 177.5 | 1.00 | C# minor | 0.00 | 2625 ms |
| score-16 | score | — | 190.3 | 0.73 | F# minor | 0.00 | 10176 ms |
| score-17 | score | — | 134.5 | 0.87 | C minor | 0.00 | 8062 ms |
| score-18 | score | — | 179.8 | 1.00 | F major | 0.00 | 12210 ms |
| score-19 | score | — | 72.0 | 1.00 | A minor | 0.01 | 3170 ms |
| score-20 | score | — | 100.0 | 1.00 | A minor | 0.00 | 6887 ms |
| score-21 | score | — | undetermined | 0.00 | G major | 0.00 | 6993 ms |
| score-22 | score | — | 91.9 | 0.70 | A major | 0.00 | 9309 ms |
| score-23 | score | — | 76.0 | 0.79 | A minor | 0.00 | 4265 ms |
| score-24 | score | — | 117.9 | 0.99 | C minor | 0.00 | 6063 ms |
| score-25 | score | — | 120.4 | 1.00 | F# minor | 0.01 | 7368 ms |
| score-26 | score | — | 122.8 | 1.00 | A# minor | 0.00 | 3543 ms |
| orchestral-27 | orchestral | — | 103.8 | 0.96 | A minor | 0.01 | 31201 ms |
| orchestral-28 | orchestral | — | 120.0 | 0.74 | A minor | 0.01 | 6681 ms |
| orchestral-29 | orchestral | — | 135.1 | 1.00 | C minor | 0.00 | 8750 ms |
| orchestral-30 | orchestral | — | 104.1 | 1.00 | E minor | 0.01 | 10519 ms |
| orchestral-31 | orchestral | — | 159.9 | 1.00 | D# major | 0.00 | 4975 ms |
| orchestral-32 | orchestral | — | 136.0 | 1.00 | B minor | 0.00 | 12218 ms |
| orchestral-33 | orchestral | — | undetermined | 0.28 | G major | 0.00 | 4268 ms |
| orchestral-34 | orchestral | — | 154.9 | 0.70 | A minor | 0.00 | 5464 ms |
| orchestral-35 | orchestral | — | 124.0 | 1.00 | E minor | 0.00 | 5100 ms |
| orchestral-36 | orchestral | — | 70.1 | 0.84 | A# minor | 0.00 | 6905 ms |
| orchestral-37 | orchestral | — | 73.2 | 1.00 | B minor | 0.00 | 8992 ms |
| orchestral-38 | orchestral | — | 95.0 | 1.00 | F major | 0.00 | 2660 ms |
| orchestral-39 | orchestral | — | 109.1 | 1.00 | G# minor | 0.00 | 2735 ms |
| orchestral-40 | orchestral | — | 110.1 | 1.00 | C minor | 0.00 | 3570 ms |

## 负例按类型

| 类型 | 条数 | 判 undetermined |
| --- | --- | --- |
