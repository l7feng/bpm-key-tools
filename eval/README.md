# eval — BPM/调性识别 S1-A 评测基线

## 职责

正式仓 `bpm-key-tool` 的**评测基线**，用于验证「先评测再定路线」裁决中的路线 A（纯 numpy 经典 DSP）基线。生成已知真值的合成信号，跑同一套指标，产出可复现报告。

本目录原为库外一次性评测原型 `D:/Ai-Files/Projects/bpm-key-eval/`，2026-11-06 迁入本仓；原目录已核验 SHA256 一致后送回收站。它是评测基线，**不是产品算法**——路线未拍板前不得作为产品内核。

| 项 | 内容 |
| --- | --- |
| 所在仓 | `D:/Ai-Files/Repos/GitHub/bpm-key-tools/` |
| 对应库内项目 | `D:/1-个人知识库_Vault/5-领域-A/06-软件制作/88-BPM调性识别工具/` |
| 唯一状态入口 | 项目 88 的 `Documentation.md`（本目录不维护项目状态） |
| 阶段 | S1 算法评测与路线拍板 · 仅 S1-A 子批 |
| 解释权 | 项目 88 的 `02-评测阶段设计.md` 定义评测集与指标口径 |

## 现状

- S1-A 已跑通：合成信号生成、BPM 自相关基线、chroma 调性模板相关、指标脚本、报告生成。
- 实测指标与失败样例见 `results/report.md`，逐条数据见 `results/results.json`，汇总见 `results/summary.json`。**指标只由脚本生成，不手工填写。**
- 本批实测（65 条合成样本）：BPM 严格准确率 72.7%、调性 top-1 91.7% / top-2 95.8%、负例 `undetermined` 触发率 50.0%。
- **判分口径修订**：真值用降号音名（`Bb`/`Eb`）、估计器只输出升号（`A#`/`D#`），字面比较会让 2 个主音必然判错。已只改 `evaluate.py` 判分归一化，不改算法与阈值；修订前 top-1 为 83.3%。详见报告「已知缺陷与本次修订」。
- **未达标项（S1 真实结论，刻意未调参修复）**：≥150 BPM 系统性低估（150/170/180 → 100.1/113.4/90.1）；`noise` 与 `atonal` 负例 4/4 未拦截，`undetermined` 触发率仅 50%；`Eb major` 五度混淆为 `A# major`。方案要求的「无调性、低置信度一律 undetermined」当前**未达标**。
- 路线 B（ONNX）未选型、未下载模型、未跑推理；真实标注样本未使用。因此「路线拍板」条件尚未满足。
- 高指标只证明合成集上管线正确，**不代表真实曲库准确率**，详见报告「效度声明」。

## 文件

| 文件 | 用途 |
| --- | --- |
| `synth.py` | 合成评测集：点击音轨（已知 BPM）、I-IV-V-I 三和弦进行（已知调性）、四类负例 |
| `dsp.py` | 经典 DSP 基线：STFT 谱通量 onset 包络 → 自相关 tempo salience → 抛物线插值 → 倍频折叠；平均谱 → chroma → Krumhansl-Schmuckler 模板相关 |
| `evaluate.py` | 跑评测、算指标、写 `results/` 三份产物 |
| `real-manifest.json` | 真实素材清单：`bpm` 数值＝有真值，`null`＝应判 undetermined 的负例，缺省＝未标注只记观测值 |
| `results/` | `results.json` / `summary.json` / `report.md`（合成集）、`report-real.md`（真实素材） |

## 真实素材评测（Rust 内核）

产品内核（`src-tauri/src/analysis.rs`）的指标由仓库内的 ignored 测试直接算出，不经过 Python：

```powershell
$env:PATH = "D:/Ai-Files/Tools/BuildEnv/sfx-tools-rust/cargo/bin;" + $env:PATH
$env:RUSTUP_HOME = "D:/Ai-Files/Tools/BuildEnv/sfx-tools-rust/rustup"
$env:CARGO_HOME = "D:/Ai-Files/Tools/BuildEnv/sfx-tools-rust/cargo"

cd D:/Ai-Files/Repos/bpm-key-tools/src-tauri
cargo test --offline eval_real_manifest -- --ignored --nocapture
```

- 默认读 `../eval/real-manifest.json`；可用 `BPM_EVAL_MANIFEST` 指到别的清单。
- 样本根目录取清单里的 `base`，可用 `BPM_REAL_BASE` 覆盖（换机器、换曲库时只改环境变量）。
- 引擎切换：`BPM_EVAL_ENGINE=model` 时走模型引擎（需 `%APPDATA%\BPM Key Tools\model-engine.json` 与权重），报告写 `results/report-real-model.md`；不设则走本地 DSP，写 `report-real.md`。两份报告用同一清单和同一套指标，可以直接并排看。
- 指标：BPM 严格（≤1 BPM）/ 容差（≤max(2 BPM, 2%)）/ 误弃 / 倍频错误 / 负例通过（按类型分组）/ 调性 top-1 / 单条耗时中位与最大。
- 报告固定写到 `results/report-real.md`，逐条附 BPM 与调性置信。40 条标注集到手后，只需往清单里补 `bpm` 与 `key` 字段，指标随即变成真实标注口径。


## 运行

```bash
cd D:/Ai-Files/Repos/GitHub/bpm-key-tools/eval
D:/Ai-Files/Tools/BuildEnv/sfx-tools-ai/Scripts/python.exe evaluate.py --out results
```

依赖只使用已安装的 numpy 2.3.4（Python 3.12.10），不安装 scipy / librosa / aubio / essentia / torch / soundfile，不下载任何模型权重。

## 边界

- 只写本目录 `results/`；不写知识库、不写音乐库标签、不改写任何原音频文件。
- 本目录随母仓 `bpm-key-tools` 管理，不单独建仓；不建远端、不提交、不推送、不打包 zip。
- aubio（GPL）与 essentia（AGPL）排除；引入 ONNX 模型前须另行审再分发许可。
- 正式代码仓已定为 `D:/Ai-Files/Repos/GitHub/bpm-key-tools/`；本目录是其评测基线，不是产品算法。
