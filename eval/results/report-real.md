# 真实素材评测报告（Rust DSP 内核）

由 `cargo test --offline eval_real_manifest -- --ignored --nocapture` 生成，指标不手工填写。
清单 `eval/real-manifest.json`：`bpm` 为数值＝有真值；`bpm: null`＝应判 undetermined 的负例；不带 `bpm`＝尚无标注，只列观测值。

## 指标

| 指标 | 结果 | 口径 |
| --- | --- | --- |
| BPM 严格 | 100.0% | 绝对误差 ≤ 1 BPM（n=1） |
| BPM 容差 | 100.0% | 误差 ≤ max(2 BPM, 2%) |
| BPM 误弃 | 0 | 有真值却判 undetermined |
| 倍频错误 | 0 | 输出约为真值 2 倍或 0.5 倍 |
| 负例通过 | 100.0% | 应判 undetermined 且确实没给数（n=18） |
| 调性 top-1 | 100.0% | 主选命中（n=1） |
| 单条耗时 | 中位 891 ms / 最大 2372 ms | 11,025 Hz 抽取后计算 |

读文件错误：0。

## 逐条

| id | 类型 | 真值 BPM | 实测 BPM | BPM 置信 | 调性 | 调性置信 | 耗时 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| click-120 | click | 120.0 | 120.0 | 0.99 | A major | 0.02 | 57 ms |
| chord-c-major | steady | undetermined | undetermined | 0.00 | C major | 0.02 | 38 ms |
| tone-96k-wav | tone | undetermined | undetermined | 0.00 | A major | 0.04 | 112 ms |
| tone-96k-aiff | tone | undetermined | undetermined | 0.00 | A major | 0.04 | 107 ms |
| tone-96k-flac | tone | undetermined | undetermined | 0.00 | A major | 0.04 | 116 ms |
| tone-44k-mp3 | tone | undetermined | undetermined | 0.00 | A major | 0.02 | 200 ms |
| tone-44k-m4a | tone | undetermined | undetermined | 0.02 | A major | 0.02 | 70 ms |
| pad-01 | score | undetermined | undetermined | 0.16 | C# minor | 0.00 | 920 ms |
| pad-02 | score | undetermined | undetermined | 0.12 | F# minor | 0.01 | 799 ms |
| pad-03 | score | undetermined | undetermined | 0.29 | F# minor | 0.01 | 821 ms |
| pad-04 | score | undetermined | undetermined | 0.13 | C# minor | 0.01 | 675 ms |
| pad-05 | score | undetermined | undetermined | 0.18 | C# minor | 0.00 | 891 ms |
| pad-06 | score | undetermined | undetermined | 0.12 | F# minor | 0.02 | 2101 ms |
| pad-07 | score | undetermined | undetermined | 0.10 | E major | 0.02 | 1810 ms |
| pad-08 | score | undetermined | undetermined | 0.11 | C# minor | 0.01 | 2196 ms |
| pad-09 | score | undetermined | undetermined | 0.12 | E major | 0.02 | 2088 ms |
| pad-10 | score | undetermined | undetermined | 0.10 | F# minor | 0.00 | 2236 ms |
| pad-11 | score | undetermined | undetermined | 0.13 | C# major | 0.00 | 1350 ms |
| pad-12 | score | undetermined | undetermined | 0.12 | E major | 0.03 | 1682 ms |
| loop-ambient-bongos | loop | — | 135.0 | 0.36 | D minor | 0.01 | 1300 ms |
| loop-backbeat | loop | — | undetermined | 0.24 | D major | 0.01 | 611 ms |
| loop-beat-one | loop | — | 159.8 | 0.30 | D major | 0.00 | 2372 ms |
| loop-bit-bit | loop | — | 100.1 | 0.83 | F major | 0.00 | 992 ms |
| loop-bollywood | loop | — | undetermined | 0.06 | C major | 0.02 | 2248 ms |
| loop-comic-game | loop | — | 90.1 | 0.51 | C minor | 0.01 | 565 ms |
| loop-forest-frolic | loop | — | 103.0 | 0.76 | F minor | 0.00 | 491 ms |
| loop-meditating | loop | — | undetermined | 0.12 | A minor | 0.03 | 2339 ms |

## 负例按类型

| 类型 | 条数 | 判 undetermined |
| --- | --- | --- |
| steady | 1 | 1 |
| tone | 5 | 5 |
| score | 12 | 12 |
