# BPM Key Tools

独立桌面工具：识别音频的 BPM（Beats Per Minute，每分钟拍数）与调性（musical key，主音与调式），自带界面、可独立分发。

## 职责

对应知识库项目 `D:/1-个人知识库_Vault/5-领域-A/06-软件制作/88-BPM调性识别工具/`。sound-workbench 音乐库只保留「链接到外部工具」入口，不内置分析、不自动写标签。

本仓是**正式产品源码仓**，与 `D:/Ai-Files/Projects/bpm-key-eval/`（一次性评测原型，已迁入本仓 `eval/`）性质不同。

## 现状（截至 2026-11-17）

| 项 | 状态 |
| --- | --- |
| 阶段 | S4 算法接入（0.4.0）：本地 DSP 内核已进产品 |
| 已完成 | 仓骨架、`eval/` 评测基线、Tauri 2 GUI（React + TS）、真实文件夹扫描、文件夹拖入加载、试听与波形预览、既有标签读取、人工判定写入（TBPM / TKEY）、三通道状态展示、操作记录、release 构建、桌面快捷方式；**本地 DSP 分析（BPM / 调性 / 置信度 / 耗时）、算法候选面板与采用、引擎选择器、批量分析** |
| 未完成 | 模型引擎（ONNX 路线，接口已留、未接）、L1 结果库落盘、L2 音乐库标签联动、40 条真实标注集评测 harness |
| 算法路线 | **已拍板：默认 DSP 先行，模型作为可选引擎**。`app_info` 里 `model` 引擎标 `ready:false`，`analyze_file` 收到非 DSP 引擎会明确报「尚未接入」。路线对比见知识库 `88-BPM调性识别工具/03-算法接入路线对比.md` |
| 远端 | `https://github.com/l7feng/bpm-key-tools.git`（`origin/main`） |

**列表与卡片里的算法值来自本机 DSP**：分析只产出候选，点「采用」填进输入框，仍需人工复核后才写回原文件；纯音 / 稳态素材 / 弱拍点会明确给 undetermined 并说明原因。

## 技术栈（已裁决）

Tauri 2 + React + Rust。算法内核与界面解耦，只留可插拔接口；界面与算法若跨语言，走独立进程桥。

## 目录

| 路径 | 用途 |
| --- | --- |
| `eval/` | 评测基线：合成信号生成、经典 DSP 基线、指标脚本与可复现报告。自 `D:/Ai-Files/Projects/bpm-key-eval/` 迁入 |
| `src/` | 前端（React 19 + TypeScript）：`App.tsx` 复核队列与扫描/试听流程、`Waveform.tsx` 波形画布、`mock.ts` 演示数据、`styles.css` 设计令牌与样式 |
| `src-tauri/` | Rust 侧：`Cargo.toml`、`tauri.conf.json`、`capabilities/main.json`、`src/main.rs`（扫描 / 波形峰值 / 元数据写回 / 拖放解析 / `app_info` / `analyze_file`）、`src/analysis.rs`（DSP 算法内核，含自写 FFT 与单元测试）、`icons/` |
| `scripts/build.ps1` | 固化便携 Rust 工具链的构建脚本 |

## 设计口径

- 强调色只有一套：冷蓝 `#6aa9e0`（描边 `#2f6ea8`、洗底 `#15283a`、hover `#8fc2ec`），与 SFX Tools、7F-IDW 设计系统 `action` 色阶一致。颜色只写在 `styles.css` 的 `:root`，其余样式引用变量，不再写死色值。
- 波形是定位工具，不是电平表：每 10 ms 取声道间最大绝对值，压缩到 480 槽，全曲峰值归一后按 0.6 次幂抬升弱段。

## 算法口径（DSP，0.4.0）

全程无第三方算法依赖：FFT 是自写的迭代 radix-2，解码复用 symphonia（0.3.0 起已在依赖里），新增 crate 数为 0。

**解耦点**：`analyze_file(path, engine)` 是唯一入口。`engine = dsp / default / auto` 走本地内核；其它值直接返回「引擎尚未接入」，所以模型路线接进来时不用改界面。

**BPM**
1. 解码：symphonia → 单声道混音 → 整数倍抽取到 11,025 Hz。
2. 起音包络：短时能量（hop ≈ rate/120，窗 3×hop）→ 半波整流差分 → 1.5 s 滑动去趋势 → 按峰值归一。
3. 稳态守卫：归一化前先算包络标准差，`< 1e-4` 直接返回空包络（纯音、持续长音、稳态噪声不会给出 BPM）。
4. 自相关：先减去包络均值再算相关系数，避免「平稳信号在所有 lag 上 r≈1」的假阳性。
5. 选峰：谐波叠加 `r[lag] + 0.5·r[2lag] + 0.33·r[3lag]`，70–185 BPM 区间先验权重 1.0（刻意不做 120 BPM 尖峰），峰值 `< 0.30` 判 undetermined，最后用抛物线插值到 0.1 BPM。

**调性**
1. 取若干分析窗做 FFT，按 3 个调音候选（A4 = 440 Hz，±0 / −40 / −50 音分）把谱峰落到 12 个音级，音分距离按高斯加权。
2. 12 音级 chroma 归一后，与「大调 / 小调 12 音级权重表」做余弦相似度（权重按相对主音的音级索引，旋转到绝对音级）。
3. top-1 / top-2 输出；音级聚集度 `< 0.07` 判 undetermined。
4. 置信度 = `(c1 − c2) / c1 × sqrt(聚集度)`，clamp 到 0–1；直白说：真实素材上这个值普遍偏低，界面就是靠它提示「必须人工复核」。

**真实素材实测（2026-11-17，`E:/Ai音乐设定集-Work` 抽样）**

| 素材 | 条数 | 结果 |
| --- | --- | --- |
| 律动 / loop | 8 | 5 条出 BPM（90.1–159.8），3 条 undetermined（峰值 0.06–0.24） |
| 铺底配乐（嫉妒 / 阴影 / 解谜等） | 12 | BPM 全 undetermined（峰值 0.10–0.29），调性给候选但置信 0–3% |
| 纯音（WAV/AIFF/FLAC/MP3/M4A） | 5 | BPM 全 undetermined，单条 51–144 ms |

结论：明确律动素材可用，无拍点素材拒绝给数是设计意图；调性权重表区分力不足，是 40 条标注集要解决的第一件事。

## 构建

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build.ps1
```

脚本会设置 `RUSTUP_HOME` / `CARGO_HOME` 指向本机便携工具链（`D:/Ai-Files/Tools/BuildEnv/sfx-tools-rust`），再执行 `npx tauri build --no-bundle`。

产物：`src-tauri/target/release/bpm-key-tool.exe`（单文件，无旁置 DLL）。

## 测试

```powershell
$env:PATH = "D:/Ai-Files/Tools/BuildEnv/sfx-tools-rust/cargo/bin;" + $env:PATH
$env:RUSTUP_HOME = "D:/Ai-Files/Tools/BuildEnv/sfx-tools-rust/rustup"
$env:CARGO_HOME = "D:/Ai-Files/Tools/BuildEnv/sfx-tools-rust/cargo"

# 单元测试（扫描/写回往返、波形峰值、算法：合成 click / C 大三和弦 / 纯音 / 白噪声）
cargo test --offline

# 真实格式冒烟（手动，样本目录里放 WAV/AIFF/FLAC/MP3/M4A）
$env:BPM_WAVE_SAMPLE_DIR = "<音频样本目录>"
cargo test --offline analyze_real_files -- --ignored --nocapture
cargo test --offline waveform_real_files -- --ignored --nocapture
```

当前基线：`8 passed / 0 failed / 2 ignored`（ignored 为两个真实样本用例）。

## 安装与运行

- 已安装位置：`D:/Ai-Files/Tools/Exe/09-BPM Key Tools（BPM 调性识别）/bpm-key-tool-v0.4.0/`（旧版 v0.3.0 / v0.2.1 保留在旁）
- 桌面快捷方式：`BPM Key Tools.lnk`
- 运行依赖系统 WebView2（Windows 10/11 通常自带）

## 运行评测

```bash
python eval/evaluate.py --out eval/results
```

实测环境：Python 3.12.10 / numpy 2.3.4。不安装 scipy／librosa／aubio／essentia／torch／soundfile，不下载任何模型权重。

产物：`eval/results/results.json`（逐条）、`summary.json`（汇总）、`report.md`（可复现报告）。

## 边界

- 只有「确认并写入」会改原文件（写 TBPM/TKEY）；「标记无法确定」「跳过」不触碰文件。
- 未复核不得触发对外写入。
- 波形解码失败只影响波形显示，不影响扫描、试听与写回；OGG / Opus 暂不支持波形（symphonia 未启用对应容器）。
- aubio（GPL）与 essentia（AGPL）排除；引入 ONNX 模型前须另行审再分发许可。
- 评测指标只对合成集成立，**不代表真实曲库准确率**；详见 `eval/results/report.md`「效度声明」。

## 对应库内入口

- 项目：`D:/1-个人知识库_Vault/5-领域-A/06-软件制作/88-BPM调性识别工具/`
- 唯一状态入口：该项目 `Documentation.md`
- 方案与评测设计：该项目 `01-独立工具方案.md`、`02-评测阶段设计.md`
