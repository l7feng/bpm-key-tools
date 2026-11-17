# engine — 模型引擎（外部 runner 侧车）

产品默认引擎是内置 DSP（`src-tauri/src/analysis.rs`，零新增 crate）。
这里是可选的**神经网络引擎**：Rust 只负责解码音频、传路径、解析 JSON，推理交给外部 runner。
这样做的三条理由：

1. 不把 onnxruntime（数十 MB 原生库）塞进安装包，exe 体积不变；
2. 权重和许可证留在使用者手里，发行包不含第三方模型；
3. runner 可以换实现（Python / 独立 exe / 另一台机器），只要遵守下面的 JSON 契约。

## 运行链路

```
exe ──decode_mono_at(22,050 Hz / 单声道)──► 临时 WAV
    ──spawn runner（配置里的 exe + args）──► stdout 一行 JSON
    ──serde 解析──► 界面「算法候选」面板（引擎标签 = MODEL）
```

未配置时：引擎下拉里「神经网络模型」为灰，面板下方给出具体原因，不会悄悄退回 DSP 冒充模型结果。

## 配置

文件：`%APPDATA%\BPM Key Tools\model-engine.json`（`BPM_MODEL_CONFIG` 可覆盖，便于测试）

```json
{
  "exe": "D:\\Ai-Files\\Tools\\BuildEnv\\sfx-tools-ai\\Scripts\\python.exe",
  "args": [
    "D:\\Ai-Files\\Repos\\bpm-key-tools\\engine\\model_runner.py",
    "--audio", "{audio}",
    "--config", "{config}"
  ],
  "timeoutSec": 180,
  "modelsDir": "C:\\Users\\<你>\\AppData\\Roaming\\BPM Key Tools\\models"
}
```

- `{audio}`：主程序解码出的临时 WAV（22.05 kHz / 16-bit / 单声道）。
- `{config}`：本配置文件自身的路径，runner 用它找 `modelsDir`。
- `timeoutSec`：单条分析超时，超时会被 kill 并报错（默认 180 秒）。
- `modelsDir`：权重与 `spec.json` 所在目录。**该目录里没有任何 `.onnx` 时，引擎显示为未就绪**。

## 输出契约

runner 必须往 stdout 打**一行** JSON（字段 camelCase，缺的字段可省略）：

```json
{"bpm": 120.0, "bpmConfidence": 0.62, "key": "A minor", "keyConfidence": 0.41,
 "keyTop2": "C major", "model": "musiCNN+tempo-net", "notes": ["…"]}
```

- `bpm` / `key` 给 `null` 表示这条判 undetermined（置信不足），不要为了「有结果」硬填。
- 主程序会把 `notes` 拼进候选面板的说明里；stderr 只用于诊断，不展示。

## 模型目录

```
models/
├─ spec.json
├─ tempo.onnx     （可选：只做 BPM）
└─ key.onnx       （可选：只做调性）
```

`spec.json` 结构见 `spec.example.json`：

- `mel`：log-mel 前端参数（`nFft` / `hop` / `nMels` / `fmin` / `fmax` / `normalize`）。
  当前实现支持 `perMel`（逐 mel 带标准化，Essentia MusiCNN 的常用预处理）与 `global`。
- `tempo.labels`：BPM 类别标签，例如 30–300 的 1 BPM 步进共 271 类；输出类别数与标签数必须一致。
- `key.labels`：24 个（12 音级 × 大小调）或 12 个音级标签（后者只能给音级，不分大小调）。
- `chunkFrames`：一次推理喂多少帧，默认 187（MusiCNN 的输入宽度）。
- `minConfidence`：tempo 峰值概率低于此值判 undetermined，默认 0.25。

## 权重从哪来（许可证按模型走，本仓库不分发）

| 模型 | 用途 | 许可证要点 |
| --- | --- | --- |
| Spotify Basic Pitch（ICASSP 2022） | 音符/起音 → 可推 chroma 与 onset | Apache-2.0，可商用；ONNX 版约数 MB |
| Beat This!（beat_this_small.onnx） | 拍点/下拍 → BPM | 上游标 MIT/Apache-2.0，落盘前请复核仓库 LICENSE |
| Essentia MusiCNN（tempo / key） | BPM 分类 + 调性分类 | 代码 AGPL、权重 CC BY-NC-SA：**仅限非商用评估**，商用需另行授权 |

放权重之前先确认许可证与你的使用场景（本项目属商用交付物，非商用许可的权重不应进发行包）。
参数与真实模型对不上时，runner 会报「类别数与 labels 不符」之类的可读错误，而不是给假结果。

## 自检

```powershell
& "D:\Ai-Files\Tools\BuildEnv\sfx-tools-ai\Scripts\python.exe" engine\model_runner.py --check --config "$env:APPDATA\BPM Key Tools\model-engine.json"
```

输出 `{"ready": true/false, "detail": "…"}`：检查 `modelsDir`、`spec.json`、权重文件、onnxruntime 是否齐全。

## 现状（2026-11-17）

- 链路已打通并有用例覆盖（`model_engine::tests::fake_runner_roundtrip`：假 runner → 解码 → 传参 → 解析）。
- 本机**尚未放入真实权重**：网络代理当时不通，下载未完成；`model_runner.py` 的前端与标签映射尚未在真实权重上标定。
- 因此「模型 vs DSP」的对比指标仍为空，不能用它下结论。
