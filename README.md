# bpm-key-tool

独立桌面工具：识别音频的 BPM（Beats Per Minute，每分钟拍数）与调性（musical key，主音与调式），自带界面、可独立分发。

## 职责

对应知识库项目 `D:/1-个人知识库_Vault/5-领域-A/06-软件制作/88-BPM调性识别工具/`。sound-workbench 音乐库只保留「链接到外部工具」入口，不内置分析、不自动写标签。

本仓是**正式产品源码仓**，与 `D:/Ai-Files/Projects/bpm-key-eval/`（一次性评测原型，已迁入本仓 `eval/`）性质不同。

## 现状（截至 2026-11-06）

| 项 | 状态 |
| --- | --- |
| 阶段 | S2 桌面工具与复核闸门，**只做与算法无关部分** |
| 已完成 | 仓骨架、`eval/` 评测基线、Tauri 2 GUI 壳（React + TS）、复核队列（演示数据）、三通道状态展示、release 构建、桌面快捷方式 |
| 未完成 | 算法内核（路线未拍板）、真实文件读取、L2 标签写入、L3 原音频元数据写入、sound-workbench 链接入口 |
| 算法路线 | **未拍板**。S1 只跑通路线 A（经典 DSP）的合成信号基线；路线 B（ONNX）与真实标注样本已暂缓 |
| 远端 | 无。本地仓，未提交、未推送 |

**当前 exe 内是固定演示数据**（4 条假候选），用于验证「候选 → 人工复核 → 分发写入」闸门链路，**不能用于判断真实音频**。界面顶部有「算法未接入 · 演示数据」标识。

**约束**：路线拍板前，本仓不得实现任何算法内核；`eval/` 只作评测基线与可插拔接口的参照，不作为产品算法。

## 技术栈（已裁决）

Tauri 2 + React + Rust。算法内核与界面解耦，只留可插拔接口；界面与算法若跨语言，走独立进程桥。

## 目录

| 路径 | 用途 |
| --- | --- |
| `eval/` | 评测基线：合成信号生成、经典 DSP 基线、指标脚本与可复现报告。自 `D:/Ai-Files/Projects/bpm-key-eval/` 迁入 |
| `src/` | 前端（React 19 + TypeScript）：`App.tsx` 复核队列、`mock.ts` 演示数据、`styles.css` |
| `src-tauri/` | Rust 侧：`Cargo.toml`、`tauri.conf.json`、`capabilities/main.json`、`src/main.rs`、`icons/` |
| `scripts/build.ps1` | 固化便携 Rust 工具链的构建脚本 |

## 构建

```powershell
powershell -ExecutionPolicy Bypass -File scripts\build.ps1
```

脚本会设置 `RUSTUP_HOME` / `CARGO_HOME` 指向本机便携工具链（`D:/Ai-Files/Tools/BuildEnv/sound-workbench-rust`），再执行 `npx tauri build --no-bundle`。

产物：`src-tauri/target/release/bpm-key-tool.exe`（约 8.5 MB，单文件，无旁置 DLL）。

## 安装与运行

- 已安装位置：`D:/Ai-Files/Tools/Exe/09-BPM调性识别/bpm-key-tool-v0.1.0/`
- 桌面快捷方式：`BPM 调性识别.lnk`
- 运行依赖系统 WebView2（Windows 10/11 通常自带）

## 运行评测

```bash
python eval/evaluate.py --out eval/results
```

实测环境：Python 3.12.10 / numpy 2.3.4。不安装 scipy／librosa／aubio／essentia／torch／soundfile，不下载任何模型权重。

产物：`eval/results/results.json`（逐条）、`summary.json`（汇总）、`report.md`（可复现报告）。

## 边界

- 不写音乐库标签、不改写任何原音频文件。写入须经人工复核闸门；S2 只做框架，不做真实写入。
- 未复核不得触发对外写入。
- 不 `git commit`、不 `git push`、不建远端、不打包 zip。
- aubio（GPL）与 essentia（AGPL）排除；引入 ONNX 模型前须另行审再分发许可。
- 评测指标只对合成集成立，**不代表真实曲库准确率**；详见 `eval/results/report.md`「效度声明」。

## 对应库内入口

- 项目：`D:/1-个人知识库_Vault/5-领域-A/06-软件制作/88-BPM调性识别工具/`
- 唯一状态入口：该项目 `Documentation.md`
- 方案与评测设计：该项目 `01-独立工具方案.md`、`02-评测阶段设计.md`
