//! 模型引擎：把神经网络推理交给外部 runner（默认 Python + onnxruntime），
//! 本进程只做三件事——读配置、解码成 22.05 kHz 单声道 WAV、传路径、收 JSON。
//!
//! 为什么走 runner 而不在 Rust 里跑 ONNX：
//! - 不新增 crate、不进安装包体积（onnxruntime 原生库动辄数十 MB）；
//! - 权重与许可证留在使用者那一侧，发行包里不含第三方模型；
//! - runner 可以换（Python / 独立 exe / 别的机器），只要遵守同一份 JSON 契约。
//!
//! 配置文件（可用环境变量 `BPM_MODEL_CONFIG` 覆盖，便于测试）：
//! `%APPDATA%\BPM Key Tools\model-engine.json`
//! ```json
//! {
//!   "exe": "D:\\Ai-Files\\Tools\\BuildEnv\\sfx-tools-ai\\Scripts\\python.exe",
//!   "args": ["D:\\Ai-Files\\Repos\\bpm-key-tools\\engine\\model_runner.py",
//!            "--audio", "{audio}", "--config", "{config}"],
//!   "timeoutSec": 180,
//!   "modelsDir": "C:\\Users\\<你>\\AppData\\Roaming\\BPM Key Tools\\models"
//! }
//! ```
//! 占位符：`{audio}` = 解码后的临时 WAV，`{config}` = 本配置文件自身的路径。

use crate::analysis::{decode_mono_at, AnalysisResult};
use serde::Deserialize;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 交给模型解码率：主流音乐模型（Basic Pitch / MusiCNN）都吃 22.05 kHz。
pub const MODEL_RATE: u32 = 22_050;

#[derive(Debug, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfig {
    /// runner 可执行文件（python.exe、或任意外部 exe / .cmd）。
    pub exe: String,
    /// 参数模板，支持 `{audio}`、`{config}` 占位符。
    #[serde(default)]
    pub args: Vec<String>,
    /// 单条分析超时，默认 180 秒。
    #[serde(default)]
    pub timeout_sec: Option<u64>,
    /// 模型目录，仅用于状态提示与缺失检查（runner 自己也会读）。
    #[serde(default)]
    pub models_dir: Option<String>,
}

/// runner 的标准输出契约（一行 JSON）。
#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct ModelOut {
    bpm: Option<f64>,
    #[serde(default)]
    bpm_confidence: Option<f32>,
    key: Option<String>,
    #[serde(default)]
    key_confidence: Option<f32>,
    #[serde(default)]
    key_top2: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    notes: Vec<String>,
    #[serde(default)]
    duration_sec: Option<f64>,
    #[serde(default)]
    sample_rate: Option<u32>,
}

fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("BPM_MODEL_CONFIG") {
        return PathBuf::from(p);
    }
    let appdata = std::env::var("APPDATA").unwrap_or_default();
    PathBuf::from(appdata)
        .join("BPM Key Tools")
        .join("model-engine.json")
}

fn load_config() -> Result<(ModelConfig, PathBuf), String> {
    let path = config_path();
    let raw = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "未配置模型引擎（找不到 {}）。写一份 model-engine.json 指向 runner 即可启用。",
            path.display()
        )
    })?;
    let cfg: ModelConfig = serde_json::from_str(&raw)
        .map_err(|e| format!("模型引擎配置格式有误（{}）：{e}", path.display()))?;
    if cfg.exe.trim().is_empty() {
        return Err("模型引擎配置里 exe 为空".into());
    }
    Ok((cfg, path))
}

/// 引擎状态：给界面决定模型引擎那一项是可选还是灰掉。
/// 只做静态检查（exe 是否存在、模型目录里有没有 .onnx），真分析时 runner 自己再校验。
pub fn status() -> (bool, String) {
    let (cfg, path) = match load_config() {
        Ok(v) => v,
        Err(err) => return (false, err),
    };
    // 绝对/相对路径要求文件存在；裸名字（如 cmd.exe、python）交给 PATH 解析。
    let exe_like_path = cfg.exe.contains('\\') || cfg.exe.contains('/');
    if exe_like_path && !Path::new(&cfg.exe).is_file() {
        return (false, format!("配置里的 runner 不存在：{}", cfg.exe));
    }
    let mut note = format!("外部 runner：{}", Path::new(&cfg.exe).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    if let Some(dir) = cfg.models_dir.as_deref() {
        let onnx = count_onnx(Path::new(dir));
        if onnx == 0 {
            // 配了模型目录却没有权重：视为未就绪，避免界面上「可点但必然失败」。
            return (
                false,
                format!("模型目录里还没有 .onnx（{dir}）；把权重和一个 spec.json 放进去后再试"),
            );
        }
        note.push_str(&format!("；模型目录 {onnx} 个 .onnx"));
    }
    let _ = path;
    (true, note)
}

fn count_onnx(dir: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map(|x| x.eq_ignore_ascii_case("onnx"))
                .unwrap_or(false)
        })
        .count()
}

/// 把 PCM 写成 16-bit 单声道 WAV（供 runner 读）。
fn write_wav(path: &Path, samples: &[f32], rate: u32) -> Result<(), String> {
    let bytes = (samples.len() * 2) as u32;
    let mut data: Vec<u8> = Vec::with_capacity(bytes as usize + 44);
    data.extend_from_slice(b"RIFF");
    data.extend_from_slice(&(36u32 + bytes).to_le_bytes());
    data.extend_from_slice(b"WAVE");
    data.extend_from_slice(b"fmt ");
    data.extend_from_slice(&16u32.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&1u16.to_le_bytes());
    data.extend_from_slice(&rate.to_le_bytes());
    data.extend_from_slice(&(rate * 2).to_le_bytes());
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&16u16.to_le_bytes());
    data.extend_from_slice(b"data");
    data.extend_from_slice(&bytes.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        data.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, data).map_err(|e| format!("写临时 WAV 失败：{e}"))
}

fn temp_wav_path() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("bpm-key-model-{}-{nanos}.wav", std::process::id()))
}

/// 跑一次 runner：返回它 stdout 里最后一行以 `{` 开头的 JSON。
fn run_runner(cfg: &ModelConfig, cfg_path: &Path, audio: &Path) -> Result<ModelOut, String> {
    let audio_text = audio.to_string_lossy().into_owned();
    let config_text = cfg_path.to_string_lossy().into_owned();
    let args: Vec<String> = cfg
        .args
        .iter()
        .map(|a| a.replace("{audio}", &audio_text).replace("{config}", &config_text))
        .collect();

    let mut cmd = Command::new(&cfg.exe);
    cmd.args(&args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW：不弹黑色控制台窗口。
        cmd.creation_flags(0x0800_0000);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("启动模型 runner 失败（{}）：{e}", cfg.exe))?;

    let timeout = Duration::from_secs(cfg.timeout_sec.unwrap_or(180).max(5));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {
                if started.elapsed() > timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "模型 runner 超时（>{} 秒），已终止",
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(format!("等待模型 runner 失败：{e}")),
        }
    };

    let mut stdout = String::new();
    if let Some(out) = child.stdout.take() {
        let mut reader = BufReader::new(out);
        let mut line = String::new();
        while let Ok(n) = reader.read_line(&mut line) {
            if n == 0 {
                break;
            }
            if line.trim_start().starts_with('{') {
                stdout = line.trim().to_string();
            }
            line.clear();
        }
    }
    let mut stderr = String::new();
    if let Some(err) = child.stderr.take() {
        let mut reader = BufReader::new(err);
        let _ = std::io::Read::read_to_string(&mut reader, &mut stderr);
    }

    if !status.success() && stdout.is_empty() {
        let tail: String = stderr.trim().lines().take(4).collect::<Vec<_>>().join(" / ");
        return Err(format!(
            "模型 runner 退出码 {:?}：{}",
            status.code(),
            if tail.is_empty() { "无输出".to_string() } else { tail }
        ));
    }
    if stdout.is_empty() {
        return Err("模型 runner 没有输出 JSON 结果".into());
    }
    serde_json::from_str::<ModelOut>(&stdout)
        .map_err(|e| format!("模型 runner 输出的 JSON 无法解析：{e}"))
}

/// 用模型引擎分析一个文件。未配置 → 返回可读错误，不退回 DSP 冒充模型结果。
pub fn analyze(path: &Path) -> Result<AnalysisResult, String> {
    let (cfg, cfg_path) = load_config()?;
    let started = Instant::now();
    let (samples, rate, duration) = decode_mono_at(path, MODEL_RATE)?;
    let wav = temp_wav_path();
    write_wav(&wav, &samples, rate)?;
    let result = run_runner(&cfg, &cfg_path, &wav);
    let _ = std::fs::remove_file(&wav);
    let out = result?;

    let mut notes: Vec<String> = out.notes;
    notes.insert(
        0,
        format!(
            "模型引擎：{}",
            out.model.as_deref().unwrap_or("外部 runner")
        ),
    );
    if out.bpm.is_none() {
        notes.push("模型未给出 BPM（判为 undetermined）".to_string());
    }
    if out.key.is_none() {
        notes.push("模型未给出调性（判为 undetermined）".to_string());
    }

    Ok(AnalysisResult {
        path: path.to_string_lossy().into_owned(),
        engine: "model".to_string(),
        bpm: out.bpm.map(|v| (v * 10.0).round() / 10.0),
        bpm_confidence: out.bpm_confidence.unwrap_or(0.0).clamp(0.0, 1.0),
        key: out.key,
        key_top2: out.key_top2,
        key_confidence: out.key_confidence.unwrap_or(0.0).clamp(0.0, 1.0),
        duration_sec: out.duration_sec.unwrap_or(duration),
        sample_rate: out.sample_rate.unwrap_or(rate),
        elapsed_ms: started.elapsed().as_millis(),
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch_dir() -> PathBuf {
        let dir = std::env::temp_dir().join("bpm-key-model-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 写一个 2 秒 440 Hz 正弦 WAV，供 analyze() 解码。
    fn write_tone(path: &Path) {
        let rate = 44_100u32;
        let total = (rate as usize) * 2;
        let samples: Vec<f32> = (0..total)
            .map(|i| {
                let t = i as f64 / rate as f64;
                (0.4 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()) as f32
            })
            .collect();
        write_wav(path, &samples, rate).unwrap();
    }

    /// 用假 runner（.cmd 直接 echo 一行 JSON）验证「配置 → 解码 → 传参 → 解析」全链路。
    #[test]
    fn fake_runner_roundtrip() {
        let dir = scratch_dir();
        let audio = dir.join("tone-for-model.wav");
        write_tone(&audio);

        let runner = dir.join("fake-runner.cmd");
        let payload = r#"{"bpm":128.0,"bpmConfidence":0.91,"key":"C major","keyConfidence":0.72,"model":"fake-net"}"#;
        std::fs::write(
            &runner,
            format!("@echo {payload}\r\nexit /b 0\r\n"),
        )
        .unwrap();

        let cfg = dir.join("model-engine.json");
        let json = format!(
            r#"{{"exe":"cmd.exe","args":["/c","{}"],"timeoutSec":30}}"#,
            runner.to_string_lossy().replace('\\', "\\\\")
        );
        std::fs::write(&cfg, json).unwrap();

        // 环境变量按进程生效：该测试独占进程内的配置路径，串行运行即可。
        std::env::set_var("BPM_MODEL_CONFIG", cfg.to_string_lossy().into_owned());
        let res = analyze(&audio).expect("假 runner 应成功");
        std::env::remove_var("BPM_MODEL_CONFIG");

        assert_eq!(res.engine, "model");
        assert_eq!(res.bpm, Some(128.0));
        assert!((res.bpm_confidence - 0.91).abs() < 1e-6);
        assert_eq!(res.key.as_deref(), Some("C major"));
        assert!(res.notes.iter().any(|n| n.contains("fake-net")));
    }

    /// 没有配置文件时，必须给出可读错误而不是悄悄退回 DSP。
    #[test]
    fn missing_config_is_reported() {
        std::env::set_var("BPM_MODEL_CONFIG", scratch_dir().join("not-there.json"));
        let err = load_config().unwrap_err();
        std::env::remove_var("BPM_MODEL_CONFIG");
        assert!(err.contains("未配置模型引擎"), "实际：{err}");
    }
}
