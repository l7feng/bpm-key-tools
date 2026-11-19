# 真实素材评测报告（引擎：神经网络模型）

由 `cargo test --offline eval_real_manifest -- --ignored --nocapture` 生成，指标不手工填写。
清单 `eval/real-manifest.json`：`bpm` 为数值＝有真值；`bpm: null`＝应判 undetermined 的负例；不带 `bpm`＝尚无标注，只列观测值。

## 指标

| 指标 | 结果 | 口径 |
| --- | --- | --- |
| BPM 严格 | 100.0% | 绝对误差 ≤ 1 BPM（n=1） |
| BPM 容差 | 100.0% | 误差 ≤ max(2 BPM, 2%) |
| BPM 误弃 | 0 | 有真值却判 undetermined |
| 倍频错误 | 0 | 输出约为真值 2 倍或 0.5 倍 |
| 负例通过 | 38.9% | 应判 undetermined 且确实没给数（n=18） |
| 调性 top-1 | 0.0% | 主选命中（n=1） |
| 单条耗时 | 中位 1286 ms / 最大 4352 ms | 11,025 Hz 抽取后计算 |

读文件错误：0。

## 逐条

| id | 类型 | 真值 BPM | 实测 BPM | BPM 置信 | 调性 | 调性置信 | 耗时 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| click-120 | click | 120.0 | 120.2 | 1.00 | A minor | 0.00 | 502 ms |
| chord-c-major | steady | undetermined | undetermined | 0.00 | E minor | 0.01 | 435 ms |
| tone-96k-wav | tone | undetermined | undetermined | 0.00 | A minor | 0.00 | 574 ms |
| tone-96k-aiff | tone | undetermined | undetermined | 0.00 | A minor | 0.00 | 439 ms |
| tone-96k-flac | tone | undetermined | undetermined | 0.00 | A minor | 0.00 | 499 ms |
| tone-44k-mp3 | tone | undetermined | undetermined | 0.00 | A minor | 0.00 | 531 ms |
| tone-44k-m4a | tone | undetermined | undetermined | 0.00 | A minor | 0.00 | 499 ms |
| pad-01 | score | undetermined | 192.2 | 0.69 | D minor | 0.00 | 1322 ms |
| pad-02 | score | undetermined | 132.4 | 0.96 | G minor | 0.00 | 1286 ms |
| pad-03 | score | undetermined | 140.2 | 1.00 | G minor | 0.00 | 1209 ms |
| pad-04 | score | undetermined | 125.3 | 0.87 | A minor | 0.00 | 994 ms |
| pad-05 | score | undetermined | 108.9 | 1.00 | D minor | 0.00 | 1235 ms |
| pad-06 | score | undetermined | 126.3 | 0.94 | G minor | 0.00 | 3355 ms |
| pad-07 | score | undetermined | 134.6 | 0.82 | E minor | 0.00 | 2452 ms |
| pad-08 | score | undetermined | 120.3 | 0.86 | A minor | 0.00 | 3307 ms |
| pad-09 | score | undetermined | 159.7 | 0.84 | E minor | 0.00 | 3055 ms |
| pad-10 | score | undetermined | 157.7 | 0.92 | C minor | 0.00 | 3271 ms |
| pad-11 | score | undetermined | 120.2 | 0.96 | C major | 0.00 | 1901 ms |
| pad-12 | score | undetermined | undetermined | 0.91 | E minor | 0.01 | 2377 ms |
| loop-ambient-bongos | loop | — | 135.2 | 0.60 | A minor | 0.00 | 1813 ms |
| loop-backbeat | loop | — | 129.8 | 1.00 | D major | 0.00 | 1057 ms |
| loop-beat-one | loop | — | undetermined | 0.21 | A minor | 0.00 | 4352 ms |
| loop-bit-bit | loop | — | 100.0 | 1.00 | C minor | 0.00 | 1526 ms |
| loop-bollywood | loop | — | 80.1 | 0.78 | C major | 0.00 | 3447 ms |
| loop-comic-game | loop | — | 90.0 | 1.00 | G minor | 0.00 | 997 ms |
| loop-forest-frolic | loop | — | 103.3 | 0.98 | F minor | 0.00 | 877 ms |
| loop-meditating | loop | — | 182.1 | 0.77 | A major | 0.00 | 3681 ms |

## 负例按类型

| 类型 | 条数 | 判 undetermined |
| --- | --- | --- |
| steady | 1 | 1 |
| tone | 5 | 5 |
| score | 12 | 1 |
