# BPM Key Tools

独立桌面工具：识别音频的 BPM（Beats Per Minute，每分钟拍数）与调性（musical key，主音与调式），自带界面、可独立分发。

## 职责

对应知识库项目 `D:/1-个人知识库_Vault/5-领域-A/06-软件制作/88-BPM调性识别工具/`。sound-workbench 音乐库只保留「链接到外部工具」入口，不内置分析、不自动写标签。

本仓是**正式产品源码仓**，与 `D:/Ai-Files/Projects/bpm-key-eval/`（一次性评测原型，已迁入本仓 `eval/`）性质不同。

## 现状（截至 2026-11-17）

| 项 | 状态 |
| --- | --- |
| 阶段 | S3 真实文件复核闸门（0.3.0），**仍只做与算法无关部分** |
| 已完成 | 仓骨架、`eval/` 评测基线、Tauri 2 GUI（React + TS）、真实文件夹扫描、文件夹拖入加载、试听与波形预览、既有标签读取、人工判定写入（TBPM / TKEY）、三通道状态展示、操作记录、release 构建、桌面快捷方式 |
| 未完成 | 算法内核（路线未拍板）、L1 结果库落盘、L2 音乐库标签联动、sound-workbench 链接入口 |
| 算法路线 | **未拍板**。S1 只跑通路线 A（经典 DSP）的合成信号基线；路线 B（ONNX）与真实标注样本已暂缓。路线对比讨论见知识库 `88-BPM调性识别工具/03-算法接入路线对比.md` |
| 远端 | 无。本地仓，未提交、未推送 |

**列表里的 BPM/调性值仍不来自算法**：只能是文件既有标签或人工判定。界面顶部有「算法未接入」标识。本轮新增的是「试听复核」体验（波形、拖入）与视觉统一，不是识别能力。

**约束**：路线拍板前，本仓不得实现任何算法内核；`eval/` 只作评测基线与可插拔接口的参照，不作为产品算法。

## 技术栈（已裁决）

Tauri 2 + React + Rust。算法内核与界面解耦，只留可插拔接口；界面与算法若跨语言，走独立进程桥。

## 目录

| 路径 | 用途 |
| --- | --- |
| `eval/` | 评测基线：合成信号生成、经典 DSP 基线、指标脚本与可复现报告。自 `D:/Ai-Files/Projects/bpm-key-eval/` 迁入 |
| `src/` | 前端（React 19 + TypeScript）：`App.tsx` 复核队列与扫描/试听流程、`Waveform.tsx` 波形画布、`mock.ts` 演示数据、`styles.css` 设计令牌与样式 |
| `src-tauri/` | Rust 侧：`Cargo.toml`、`tauri.conf.json`、`capabilities/main.json`、`src/main.rs`（扫描 / 波形峰值 / 元数据写回 / 拖放解析）、`icons/` |
| `scripts/build.ps1` | 固化便携 Rust 工具链的构建脚本 |

## 设计口径

- 强调色只有一套：冷蓝 `#6aa9e0`（描边 `#2f6ea8`、洗底 `#15283a`、hover `#8fc2ec`），与 SFX Tools、7F-IDW 设计系统 `action` 色阶一致。颜色只写在 `styles.css` 的 `:root`，其余样式引用变量，不再写死色值。
- 波形是定位工具，不是电平表：每 10 ms 取声道间最大绝对值，压缩到 480 槽，全曲峰值归一后按 0.6 次幂抬升弱段。

## 构建

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build.ps1
```

脚本会设置 `RUSTUP_HOME` / `CARGO_HOME` 指向本机便携工具链（`D:/Ai-Files/Tools/BuildEnv/sfx-tools-rust`），再执行 `npx tauri build --no-bundle`。

产物：`src-tauri/target/release/bpm-key-tool.exe`（单文件，无旁置 DLL）。

## 测试

```powershell
# 单元测试（WAV 扫描/写回往返、波形峰值）
cargo test

# 真实格式波形解码（手动，需先准备样本目录）
$env:BPM_WAVE_SAMPLE_DIR = "<音频样本目录>"
cargo test waveform_real_files -- --ignored --nocapture
```

## 安装与运行

- 已安装位置：`D:/Ai-Files/Tools/Exe/09-BPM Key Tools（BPM 调性识别）/bpm-key-tool-v0.3.0/`（旧版 v0.2.1 保留在旁）
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
