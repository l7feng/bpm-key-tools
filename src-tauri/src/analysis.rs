//! 算法内核：经典 DSP（默认引擎）。
//!
//! 设计口径（对齐 03-算法接入路线对比.md）：
//! - 只用 Rust 原生计算，不引入第三方 FFT/模型依赖，打包体积几乎不增。
//! - BPM：短时能量包络 → 半波整流差分 → 自相关 → 谐波叠加（梳状打分）→ 抛物线插值细化。
//!   刻意不使用 120 BPM 速度先验，避免快歌被拉到 100 附近。
//! - 调性：1024 点 FFT 幅度谱 → 音级映射（含 ±40 音分调音候选）→ Krumhansl-Kessler 模板相关，
//!   同时给出 top-1 / top-2，供人工复核选第二个候选。
//! - 低置信度直接返回 None（undetermined），不伪造结果。
//!
//! 模型引擎（ONNX）本轮不接入：`engine = "model"` 会返回可读错误，界面据此展示「未安装」。

use serde::Serialize;
use std::path::Path;
use std::time::Instant;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// 分析用的重采样目标：11.025 kHz 足够覆盖包络与调性基频，且把计算量压到 1/4。
const TARGET_RATE: u32 = 11_025;
/// 包络帧率（帧/秒）。120 Hz 时 120 BPM 的拍间隔约 60 帧，配合插值可达 ±1 BPM。
const ENV_HZ: f64 = 120.0;
/// BPM 搜索范围。
const BPM_MIN: f64 = 40.0;
const BPM_MAX: f64 = 210.0;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    pub path: String,
    pub engine: String,
    /// 40–210 之外的估计不可信，直接给 None。
    pub bpm: Option<f64>,
    pub bpm_confidence: f32,
    pub key: Option<String>,
    pub key_top2: Option<String>,
    pub key_confidence: f32,
    pub duration_sec: f64,
    pub sample_rate: u32,
    pub elapsed_ms: u128,
    /// 给界面的可读说明：为什么是 None、做了哪些取舍。
    pub notes: Vec<String>,
}

/// 解码成单声道、重采样到 TARGET_RATE 的 PCM。
fn decode_mono(path: &Path) -> Result<(Vec<f32>, u32, f64), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("打开文件失败：{e}"))?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| format!("无法解析该音频容器：{e}"))?;
    let mut format = probed.format;
    let track = format
        .default_track()
        .ok_or_else(|| "音频里没有可用轨道".to_string())?;
    let track_id = track.id;
    let source_rate = track.codec_params.sample_rate.unwrap_or(0);
    let codec_params = track.codec_params.clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("该格式暂不支持分析：{e}"))?;

    let mut out: Vec<f32> = Vec::new();
    let mut decim = 1usize;
    let mut decim_acc = 0f64;
    let mut decim_n = 0usize;
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut rate = source_rate;

    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(err) => return Err(format!("读取音频包失败：{err}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(buf) => buf,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(SymphoniaError::IoError(err))
                if err.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                break;
            }
            Err(err) => return Err(format!("解码失败：{err}")),
        };
        let spec = *decoded.spec();
        let frames = decoded.frames();
        if frames == 0 {
            continue;
        }
        if sample_buf.is_none() {
            if spec.rate > 0 {
                rate = spec.rate;
                decim = ((spec.rate as f64 / TARGET_RATE as f64).round() as usize).max(1);
            }
            sample_buf = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
        }
        let buffer = sample_buf.as_mut().expect("样本缓冲已初始化");
        buffer.copy_interleaved_ref(decoded);
        let channels = spec.channels.count();
        let samples = buffer.samples();
        for frame in 0..frames {
            let base = frame * channels;
            let mut sum = 0f32;
            for ch in 0..channels {
                sum += samples[base + ch];
            }
            decim_acc += (sum / channels as f32) as f64;
            decim_n += 1;
            if decim_n >= decim {
                out.push((decim_acc / decim as f64) as f32);
                decim_acc = 0.0;
                decim_n = 0;
            }
        }
    }
    if decim_n > 0 {
        out.push((decim_acc / decim_n as f64) as f32);
    }
    if out.is_empty() {
        return Err("没有解码出音频数据".to_string());
    }
    let out_rate = (rate as usize / decim) as u32;
    let duration = out.len() as f64 / out_rate.max(1) as f64;
    Ok((out, out_rate, duration))
}

/// 短时能量包络 → 半波整流差分 → 去慢趋势 → 归一。
fn onset_envelope(samples: &[f32], rate: u32) -> Vec<f32> {
    let hop = ((rate as f64) / ENV_HZ).round().max(1.0) as usize;
    let win = hop * 3;
    if samples.len() <= win {
        return Vec::new();
    }
    let frames = (samples.len() - win) / hop + 1;
    let mut energy = Vec::with_capacity(frames);
    for i in 0..frames {
        let start = i * hop;
        let mut acc = 0f64;
        for s in &samples[start..start + win] {
            acc += (*s as f64) * (*s as f64);
        }
        energy.push((acc / win as f64).sqrt() as f32);
    }
    // 一阶差分并半波整流：只保留「变响」的部分，等同最朴素的 onset strength。
    let mut env = vec![0f32; energy.len()];
    for i in 1..energy.len() {
        let d = energy[i] - energy[i - 1];
        env[i] = if d > 0.0 { d } else { 0.0 };
    }
    // 去掉 1.5 s 尺度的慢趋势，避免整段音量起伏主导自相关。
    let trend_n = (ENV_HZ * 1.5) as usize;
    let mut trend = 0f64;
    let mut out = vec![0f32; env.len()];
    for i in 0..env.len() {
        trend += env[i] as f64;
        if i >= trend_n {
            trend -= env[i - trend_n] as f64;
        }
        let avg = trend / (trend_n.min(i + 1)) as f64;
        let v = env[i] as f64 - avg;
        out[i] = if v > 0.0 { v as f32 } else { 0.0 };
    }
    let max = out.iter().copied().fold(0f32, f32::max);
    if max <= 0.0 {
        return Vec::new();
    }
    // 平稳信号（纯音、稳态噪声、持续长音）几乎没有起音，包络的标准差会非常小；
    // 这种情况不归一化放大，直接返回空包络，交给上层按 undetermined 处理。
    let mean = out.iter().map(|v| *v as f64).sum::<f64>() / out.len() as f64;
    let var = out
        .iter()
        .map(|v| {
            let d = *v as f64 - mean;
            d * d
        })
        .sum::<f64>()
        / out.len() as f64;
    if var.sqrt() < 1e-4 {
        return Vec::new();
    }
    for v in out.iter_mut() {
        *v /= max;
    }
    out
}

/// 归一化自相关：先把包络去均值，避免「平稳信号 r≈1 于所有 lag」的假阳性。
fn autocorrelation(env: &[f32], min_lag: usize, max_lag: usize) -> Vec<f64> {
    let len = env.len();
    let mean = env.iter().map(|v| *v as f64).sum::<f64>() / len.max(1) as f64;
    let mut r = vec![0f64; max_lag + 1];
    for lag in min_lag..=max_lag.min(len.saturating_sub(1)) {
        let n = len - lag;
        if n < 20 {
            continue;
        }
        let mut dot = 0f64;
        let mut e1 = 0f64;
        let mut e2 = 0f64;
        for i in 0..n {
            let a = env[i] as f64 - mean;
            let b = env[i + lag] as f64 - mean;
            dot += a * b;
            e1 += a * a;
            e2 += b * b;
        }
        if e1 > 0.0 && e2 > 0.0 {
            r[lag] = dot / (e1.sqrt() * e2.sqrt());
        }
    }
    r
}

/// 谐波叠加打分：把 lag、2×lag、3×lag 的自相关按 1 / 0.5 / 0.33 加权，
/// 让「真拍间隔」压过它的两倍（减半错误）与三倍。
fn comb_score(r: &[f64], lag: usize) -> f64 {
    let mut score = r.get(lag).copied().unwrap_or(0.0);
    if let Some(v) = r.get(lag * 2) {
        score += 0.5 * v;
    }
    if let Some(v) = r.get(lag * 3) {
        score += 0.33 * v;
    }
    score
}

fn octave_prior(bpm: f64) -> f64 {
    // 只做宽区间偏好：70–185 视为常用区，两侧递减轻罚；不设 120 尖峰。
    if (70.0..=185.0).contains(&bpm) {
        1.0
    } else if bpm < 70.0 {
        (bpm / 70.0).clamp(0.5, 1.0) * 0.9
    } else {
        0.9
    }
}

fn detect_bpm(env: &[f32]) -> (Option<f64>, f32, Vec<String>) {
    let mut notes = Vec::new();
    if env.is_empty() {
        notes.push("未检测到明显起音（纯音或稳态素材），BPM 记为 undetermined".to_string());
        return (None, 0.0, notes);
    }
    if env.len() < 200 {
        notes.push("音频过短，无法估计 BPM".to_string());
        return (None, 0.0, notes);
    }
    let min_lag = ((ENV_HZ * 60.0) / BPM_MAX).round().max(2.0) as usize;
    let max_lag = ((ENV_HZ * 60.0) / BPM_MIN).round() as usize;
    let r = autocorrelation(env, min_lag, max_lag);

    // 在候选 lag 上打分并记录，供八度决策使用。
    let mut best: Option<(usize, f64)> = None;
    for lag in min_lag..=max_lag {
        let bpm = 60.0 * ENV_HZ / lag as f64;
        if !(BPM_MIN..=BPM_MAX).contains(&bpm) {
            continue;
        }
        let score = comb_score(&r, lag) * octave_prior(bpm);
        if best.map_or(true, |(_, s)| score > s) {
            best = Some((lag, score));
        }
    }
    let (lag, score) = match best {
        Some(v) => v,
        None => {
            notes.push("未找到可用拍点周期".to_string());
            return (None, 0.0, notes);
        }
    };

    // 八度检查：比较 lag 与 lag/2、lag×2，取加权后更高者（真拍 vs 倍频）。
    let mut chosen = lag;
    for factor in [0.5f64, 2.0f64] {
        let alt = (lag as f64 * factor).round() as usize;
        if alt < min_lag || alt > max_lag {
            continue;
        }
        let bpm = 60.0 * ENV_HZ / alt as f64;
        let alt_score = comb_score(&r, alt) * octave_prior(bpm);
        if alt_score > score * 1.08 {
            notes.push(format!(
                "八度修正：{} → {} BPM",
                60.0 * ENV_HZ / lag as f64,
                60.0 * ENV_HZ / alt as f64
            ));
            chosen = alt;
            break;
        }
    }

    let peak = r[chosen];
    if peak < 0.30 {
        notes.push(format!(
            "拍点周期性弱（峰值 {:.2}），按 undetermined 处理",
            peak
        ));
        return (None, peak.max(0.0).clamp(0.0, 1.0) as f32, notes);
    }

    // 抛物线插值，把 lag 从整数帧细化到亚帧，才可能达到 ±1 BPM。
    let refined = if chosen > min_lag && chosen < max_lag {
        let y0 = r[chosen - 1];
        let y1 = r[chosen];
        let y2 = r[chosen + 1];
        let denom = y0 - 2.0 * y1 + y2;
        if denom.abs() > 1e-9 {
            let delta = 0.5 * (y0 - y2) / denom;
            if delta.abs() <= 1.0 {
                chosen as f64 + delta
            } else {
                chosen as f64
            }
        } else {
            chosen as f64
        }
    } else {
        chosen as f64
    };

    let bpm = 60.0 * ENV_HZ / refined;
    notes.push(format!("自相关峰值 {:.2}，包络帧率 {:.0} Hz", peak, ENV_HZ));
    (Some(bpm), peak.max(0.0).clamp(0.0, 1.0) as f32, notes)
}

const KEY_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

/// 调性打分用的音阶权重（比模板相关更好解释，也不依赖某个模板表的排列顺序）。
/// 索引是「相对主音的音级」，尺度内音级给正权重，尺度外给很低的底噪权重。
const MAJOR_WEIGHTS: [f64; 12] = [
    1.00, 0.10, 0.55, 0.10, 0.75, 0.45, 0.10, 0.85, 0.10, 0.50, 0.10, 0.40,
];
const MINOR_WEIGHTS: [f64; 12] = [
    1.00, 0.10, 0.50, 0.78, 0.10, 0.45, 0.10, 0.88, 0.55, 0.10, 0.42, 0.10,
];

/// 把「相对主音」的权重旋转到绝对音级上。
fn rotated(weights: &[f64; 12], tonic: usize) -> [f64; 12] {
    let mut out = [0f64; 12];
    for i in 0..12 {
        out[(i + tonic) % 12] = weights[i];
    }
    out
}

/// 加权匹配度：Σ chroma × weights，再除以权重和（各调一致，可直接比较）。
/// 余弦相似度：chroma 与调性权重向量都按各自模长归一，
/// 这样不同调性之间比的是「形状像不像」，不会被绝对值尺度糊平。
fn fit_score(chroma: &[f64; 12], weights: &[f64; 12]) -> f64 {
    let mut num = 0f64;
    let mut da = 0f64;
    let mut db = 0f64;
    for i in 0..12 {
        num += chroma[i] * weights[i];
        da += chroma[i] * chroma[i];
        db += weights[i] * weights[i];
    }
    if da > 0.0 && db > 0.0 {
        num / (da.sqrt() * db.sqrt())
    } else {
        0.0
    }
}

/// 迭代 radix-2 FFT（自写，避免引入 rustfft 依赖）。
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two());
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let angle = -2.0 * std::f64::consts::PI / len as f64;
        let wlen_re = angle.cos();
        let wlen_im = angle.sin();
        let mut i = 0usize;
        while i < n {
            let mut w_re = 1.0f64;
            let mut w_im = 0.0f64;
            for k in 0..len / 2 {
                let u_re = re[i + k];
                let u_im = im[i + k];
                let v_re = re[i + k + len / 2] * w_re - im[i + k + len / 2] * w_im;
                let v_im = re[i + k + len / 2] * w_im + im[i + k + len / 2] * w_re;
                re[i + k] = u_re + v_re;
                im[i + k] = u_im + v_im;
                re[i + k + len / 2] = u_re - v_re;
                im[i + k + len / 2] = u_im - v_im;
                let next_re = w_re * wlen_re - w_im * wlen_im;
                let next_im = w_re * wlen_im + w_im * wlen_re;
                w_re = next_re;
                w_im = next_im;
            }
            i += len;
        }
        len <<= 1;
    }
}

fn detect_key(samples: &[f32], rate: u32) -> (Option<String>, Option<String>, f32, Vec<String>) {
    let mut notes = Vec::new();
    let n_fft = 1024usize;
    let hop = 512usize;
    if samples.len() < n_fft {
        notes.push("音频过短，无法估计调性".to_string());
        return (None, None, 0.0, notes);
    }
    let bin_hz = rate as f64 / n_fft as f64;
    let lo_bin = ((60.0 / bin_hz).ceil() as usize).max(1);
    let hi_bin = ((1600.0 / bin_hz).floor() as usize).min(n_fft / 2);
    // 三个 ±40 音分的调音候选，取「音级聚集度」最高的那个。
    let offsets = [-40.0f64, 0.0, 40.0];
    let mut chroma = [[0f64; 12]; 3];
    let mut window = vec![0f64; n_fft];
    for i in 0..n_fft {
        window[i] = 0.5 - 0.5 * ((2.0 * std::f64::consts::PI * i as f64) / n_fft as f64).cos();
    }

    let mut frames = 0usize;
    let mut re = vec![0f64; n_fft];
    let mut im = vec![0f64; n_fft];
    let mut start = 0usize;
    while start + n_fft <= samples.len() {
        for i in 0..n_fft {
            re[i] = samples[start + i] as f64 * window[i];
            im[i] = 0.0;
        }
        fft(&mut re, &mut im);
        let mut frame_energy = 0f64;
        let mut frame_chroma = [[0f64; 12]; 3];
        for bin in lo_bin..=hi_bin {
            let mag = (re[bin] * re[bin] + im[bin] * im[bin]).sqrt();
            if mag <= 0.0 {
                continue;
            }
            let freq = bin as f64 * bin_hz;
            let midi = 69.0 + 12.0 * (freq / 440.0).log2();
            for (k, off) in offsets.iter().enumerate() {
                let p = midi + off / 100.0;
                let pc = ((p.round() as i64).rem_euclid(12)) as usize;
                // 靠近音级中心的谱峰给更高权重，抑制音级边界噪声。
                let dist = (p - p.round()).abs();
                let weight = mag * (1.0 - dist).max(0.0).powi(2);
                frame_chroma[k][pc] += weight;
                frame_energy += weight;
            }
        }
        if frame_energy > 0.0 {
            for k in 0..3 {
                let sum: f64 = frame_chroma[k].iter().sum();
                if sum > 0.0 {
                    for i in 0..12 {
                        chroma[k][i] += frame_chroma[k][i] / sum;
                    }
                }
            }
            frames += 1;
        }
        start += hop;
    }
    if frames == 0 {
        notes.push("没有可用的分析帧".to_string());
        return (None, None, 0.0, notes);
    }

    // 选聚集度最高（平方和最大）的调音候选。
    let mut best_k = 0usize;
    let mut best_peak = -1.0f64;
    for k in 0..3 {
        let norm: f64 = chroma[k].iter().sum();
        if norm <= 0.0 {
            continue;
        }
        let mut peak = 0f64;
        for i in 0..12 {
            let v = chroma[k][i] / norm;
            peak += v * v;
        }
        if peak > best_peak {
            best_peak = peak;
            best_k = k;
        }
    }
    let norm: f64 = chroma[best_k].iter().sum();
    if norm <= 0.0 {
        notes.push("chroma 为空，按 undetermined 处理".to_string());
        return (None, None, 0.0, notes);
    }
    let mut vec = [0f64; 12];
    for i in 0..12 {
        vec[i] = chroma[best_k][i] / norm;
    }
    // 音级聚集度太低（噪声、无调性素材）就不给结论。
    if best_peak < 0.07 {
        notes.push(format!(
            "音级聚集度低（{:.3}），按 undetermined 处理",
            best_peak
        ));
        return (None, None, 0.0, notes);
    }

    let mut scores: Vec<(f64, usize, bool)> = Vec::new(); // (score, tonic, is_major)
    for tonic in 0..12 {
        scores.push((fit_score(&vec, &rotated(&MAJOR_WEIGHTS, tonic)), tonic, true));
        scores.push((fit_score(&vec, &rotated(&MINOR_WEIGHTS, tonic)), tonic, false));
    }
    scores.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let (c1, t1, major1) = scores[0];
    let (c2, t2, major2) = scores.get(1).copied().unwrap_or((0.0, 0, true));
    let label = |tonic: usize, major: bool| {
        format!(
            "{} {}",
            KEY_NAMES[tonic],
            if major { "major" } else { "minor" }
        )
    };
    notes.push(format!(
        "调音候选 {:+.0} 音分，音级聚集度 {:.3}，匹配度 {:.3}",
        offsets[best_k], best_peak, c1
    ));
    notes.push(format!("次优候选：{}（{:.3}）", label(t2, major2), c2));
    // 置信度用「第一名相对第二名领先多少」，再按音级聚集度缩放。
    let margin = if c1 > 0.0 { (c1 - c2).max(0.0) / c1 } else { 0.0 };
    let conf = (margin * best_peak.sqrt()).clamp(0.0, 1.0) as f32;
    (Some(label(t1, major1)), Some(label(t2, major2)), conf, notes)
}

/// 分析入口：engine = "dsp" 走本地算法；"model" 明确报未接入。
pub fn analyze_file(path: &Path, engine: &str) -> Result<AnalysisResult, String> {
    let started = Instant::now();
    let engine = engine.trim().to_ascii_lowercase();
    if engine != "dsp" && engine != "default" && engine != "auto" {
        return Err(format!(
            "引擎「{engine}」尚未接入：模型路线需先确认权重许可与打包体积，当前只有 DSP 可用"
        ));
    }
    let (samples, rate, duration) = decode_mono(path)?;
    let env = onset_envelope(&samples, rate);
    let (bpm, bpm_conf, mut notes) = detect_bpm(&env);
    let (key, key_top2, key_conf, key_notes) = detect_key(&samples, rate);
    notes.extend(key_notes);

    Ok(AnalysisResult {
        path: path.to_string_lossy().into_owned(),
        engine: "dsp".to_string(),
        bpm: bpm.map(|v| (v * 10.0).round() / 10.0),
        bpm_confidence: bpm_conf,
        key,
        key_top2,
        key_confidence: key_conf,
        duration_sec: duration,
        sample_rate: rate,
        elapsed_ms: started.elapsed().as_millis(),
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::var("BPM_ANALYSIS_TEST_DIR")
            .unwrap_or_else(|_| std::env::temp_dir().to_string_lossy().into_owned());
        let path = PathBuf::from(dir).join(name);
        path
    }

    /// 写一个 16-bit PCM 单声道 WAV。
    fn write_wav(path: &Path, samples: &[f32], rate: u32) {
        let mut data: Vec<u8> = Vec::new();
        let bytes = (samples.len() * 2) as u32;
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
        std::fs::write(path, data).unwrap();
    }

    /// 合成「拍点清晰」的素材：每拍一段 30 ms 的衰减脉冲，模拟打点明确的配乐。
    fn click_track(bpm: f64, rate: u32, secs: f64) -> Vec<f32> {
        let total = (rate as f64 * secs) as usize;
        let mut out = vec![0f32; total];
        let period = 60.0 / bpm;
        let burst = (rate as f64 * 0.03) as usize;
        let mut t = 0.0f64;
        while t < secs {
            let start = (t * rate as f64) as usize;
            for i in 0..burst {
                if start + i >= total {
                    break;
                }
                let env = 1.0 - (i as f64 / burst as f64);
                let phase = 2.0 * std::f64::consts::PI * 500.0 * (i as f64 / rate as f64);
                out[start + i] += (env * phase.sin()) as f32 * 0.8;
            }
            t += period;
        }
        out
    }

    /// 合成三和弦，用于检查调性模板是否给出正确的 top-1。
    fn triad(root_hz: f64, third_hz: f64, fifth_hz: f64, rate: u32, secs: f64) -> Vec<f32> {
        let total = (rate as f64 * secs) as usize;
        let mut out = vec![0f32; total];
        for i in 0..total {
            let t = i as f64 / rate as f64;
            let v = (2.0 * std::f64::consts::PI * root_hz * t).sin()
                + (2.0 * std::f64::consts::PI * third_hz * t).sin()
                + (2.0 * std::f64::consts::PI * fifth_hz * t).sin();
            out[i] = (v / 3.0) as f32;
        }
        out
    }

    #[test]
    fn bpm_of_click_track_120() {
        let path = scratch("bpm120.wav");
        let rate = 44_100;
        write_wav(&path, &click_track(120.0, rate, 12.0), rate);
        let r = analyze_file(&path, "dsp").unwrap();
        let bpm = r.bpm.expect("应给出 BPM");
        assert!((bpm - 120.0).abs() <= 1.5, "120 BPM 实测 {bpm}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn bpm_of_click_track_fast() {
        let path = scratch("bpm150.wav");
        let rate = 44_100;
        write_wav(&path, &click_track(150.0, rate, 12.0), rate);
        let r = analyze_file(&path, "dsp").unwrap();
        let bpm = r.bpm.expect("应给出 BPM");
        assert!((bpm - 150.0).abs() <= 1.5, "150 BPM 实测 {bpm}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn key_of_c_major_triad() {
        let path = scratch("cmajor.wav");
        let rate = 44_100;
        write_wav(&path, &triad(261.63, 329.63, 391.99, rate, 6.0), rate);
        let r = analyze_file(&path, "dsp").unwrap();
        let key = r.key.expect("应给出调性");
        assert!(key.starts_with('C') && key.ends_with("major"), "C 大三和弦实测 {key}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn noise_is_undetermined() {
        let path = scratch("noise.wav");
        let rate = 44_100;
        let mut seed = 12345u64;
        let samples: Vec<f32> = (0..(rate as usize * 6))
            .map(|_| {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((seed >> 33) as f32 / u32::MAX as f32 - 0.5) * 2.0 * 0.3
            })
            .collect();
        write_wav(&path, &samples, rate);
        let r = analyze_file(&path, "dsp").unwrap();
        assert!(r.bpm.is_none() || r.bpm_confidence < 0.4, "白噪声 BPM 置信度 {:?}", r.bpm);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn steady_tone_has_no_bpm() {
        let path = scratch("steady.wav");
        let rate = 44_100;
        let total = (rate as f64 * 6.0) as usize;
        let samples: Vec<f32> = (0..total)
            .map(|i| {
                let t = i as f64 / rate as f64;
                (0.4 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()) as f32
            })
            .collect();
        write_wav(&path, &samples, rate);
        let r = analyze_file(&path, "dsp").unwrap();
        assert!(r.bpm.is_none(), "纯音不应给出 BPM，实测 {:?}", r.bpm);
        let _ = std::fs::remove_file(path);
    }

    /// 真实格式冒烟（手动跑）：样本目录由 BPM_WAVE_SAMPLE_DIR 指定。
    #[test]
    #[ignore]
    fn analyze_real_files() {
        let dir = match std::env::var("BPM_WAVE_SAMPLE_DIR") {
            Ok(v) => v,
            Err(_) => {
                println!("未设置 BPM_WAVE_SAMPLE_DIR，跳过");
                return;
            }
        };
        let mut ok = 0;
        let mut err = 0;
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            if !path.is_file() {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            match analyze_file(&path, "dsp") {
                Ok(r) => {
                    ok += 1;
                    println!(
                        "OK {name} dur={:.1}s bpm={:?}({:.2}) key={:?}/{:?}({:.2}) {}ms notes={:?}",
                        r.duration_sec,
                        r.bpm,
                        r.bpm_confidence,
                        r.key,
                        r.key_top2,
                        r.key_confidence,
                        r.elapsed_ms,
                        r.notes
                    );
                }
                Err(e) => {
                    err += 1;
                    println!("ERR {name} {e}");
                }
            }
        }
        println!("样本：OK {ok} / ERR {err}");
        assert_eq!(err, 0, "全部样本都应能分析");
    }
}
