#![windows_subsystem = "windows"]

mod analysis;

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::BoundTaggedFile;
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::{Tag, TagType};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// 供前端查询应用与算法状态。BPM/调性识别已接入本地 DSP 引擎（默认）；
/// 模型引擎预留接口，未接入前会返回可读错误。
#[tauri::command]
fn app_info() -> serde_json::Value {
    serde_json::json!({
        "name": "BPM Key Tools",
        "version": env!("CARGO_PKG_VERSION"),
        "algorithm": "dsp",
        "engines": [
            { "id": "dsp", "label": "本地 DSP", "ready": true, "note": "经典信号处理：拍点自相关 + 调性音阶匹配" },
            { "id": "model", "label": "神经网络模型", "ready": false, "note": "需先确认权重许可与打包体积，未接入" }
        ],
        "mock": false,
        "stage": "S3-real-audit",
        "capabilities": ["scan", "drag_drop", "playback", "waveform", "metadata_write", "analyze"]
    })
}

/// 弹出系统对话框选择一个文件夹；取消时返回 None。
#[tauri::command]
async fn pick_folder() -> Option<String> {
    tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("选择要复核的音频文件夹")
            .pick_folder()
            .map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .ok()
    .flatten()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct TrackFile {
    path: String,
    file_name: String,
    duration_sec: f64,
    size_bytes: u64,
    bpm_existing: Option<String>,
    key_existing: Option<String>,
    title: Option<String>,
    artist: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ScanResult {
    root: String,
    tracks: Vec<TrackFile>,
    errors: Vec<String>,
}

const AUDIO_EXTS: [&str; 9] = ["wav", "wave", "aif", "aiff", "flac", "mp3", "m4a", "ogg", "opus"];
const MAX_DEPTH: usize = 8;
const MAX_FILES: usize = 20000;

fn is_audio(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

fn collect_audio_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_audio_files(&path, depth + 1, out);
        } else if is_audio(&path) {
            out.push(path);
        }
    }
}

fn read_track(path: &Path) -> Result<TrackFile, String> {
    let meta = std::fs::metadata(path).map_err(|e| e.to_string())?;
    let tagged = Probe::open(path)
        .and_then(|p| p.read())
        .map_err(|e| format!("{}: {}", path.display(), e))?;
    let duration_sec = tagged.properties().duration().as_secs_f64();
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
    let (bpm_existing, key_existing, title, artist) = match tag {
        Some(tag) => (
            // ID3v2 只有整数 TBPM；Vorbis/MP4 等用小数 BPM，两个键都读。
            tag.get_string(&ItemKey::IntegerBpm)
                .or_else(|| tag.get_string(&ItemKey::Bpm))
                .map(str::to_owned),
            tag.get_string(&ItemKey::InitialKey).map(str::to_owned),
            tag.get_string(&ItemKey::TrackTitle).map(str::to_owned),
            tag.get_string(&ItemKey::TrackArtist).map(str::to_owned),
        ),
        None => (None, None, None, None),
    };
    Ok(TrackFile {
        path: path.to_string_lossy().into_owned(),
        file_name: path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        duration_sec,
        size_bytes: meta.len(),
        bpm_existing,
        key_existing,
        title,
        artist,
    })
}

/// 递归扫描文件夹里的音频文件，读取时长、体积与既有 BPM/调性标签（只读，不写任何文件）。
#[tauri::command]
async fn scan_folder(root: String) -> Result<ScanResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let base = PathBuf::from(&root);
        if !base.is_dir() {
            return Err(format!("不是文件夹：{}", root));
        }
        let started = Instant::now();
        let mut files = Vec::new();
        collect_audio_files(&base, 0, &mut files);
        files.sort();
        let mut tracks = Vec::new();
        let mut errors = Vec::new();
        for path in &files {
            match read_track(path) {
                Ok(track) => tracks.push(track),
                Err(err) => errors.push(err),
            }
        }
        if errors.len() > 20 {
            let dropped = errors.len() - 20;
            errors.truncate(20);
            errors.push(format!("……另有 {} 个文件读取失败，未逐条列出", dropped));
        }
        println!(
            "scan {}: {} 个音频，{} 个读取失败，用时 {:?}",
            root,
            tracks.len(),
            errors.len(),
            started.elapsed()
        );
        Ok(ScanResult { root, tracks, errors })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteRequest {
    path: String,
    /// BPM 文本；空字符串表示不写 BPM。
    bpm: String,
    /// 调性文本；空字符串表示不写调性。
    key: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WriteResult {
    path: String,
    bpm_written: Option<String>,
    key_written: Option<String>,
    /// 写入前的旧标签值，供操作记录留档。
    bpm_before: Option<String>,
    key_before: Option<String>,
}

/// 把复核确认的 BPM/调性写回原音频文件的元数据标签（TBPM / TKEY）。
/// 直接写原文件、不生成副本；写前先读取旧值返回给前端留档。
#[tauri::command]
async fn write_metadata(request: WriteRequest) -> Result<WriteResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path = PathBuf::from(&request.path);
        if !path.is_file() {
            return Err(format!("文件不存在：{}", request.path));
        }
        let bpm_trim = request.bpm.trim();
        let key_trim = request.key.trim();
        if bpm_trim.is_empty() && key_trim.is_empty() {
            return Err("BPM 与调性均为空，没有可写入的值".into());
        }
        let handle = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| format!("打开文件失败（可能被其他程序占用）：{}", e))?;
        let mut bound = BoundTaggedFile::read_from(handle, ParseOptions::new())
            .map_err(|e| format!("读取失败：{}", e))?;
        let bpm_before = bound
            .primary_tag()
            .and_then(|tag| {
                tag.get_string(&ItemKey::IntegerBpm)
                    .or_else(|| tag.get_string(&ItemKey::Bpm))
            })
            .map(str::to_owned);
        let key_before = bound
            .primary_tag()
            .and_then(|tag| tag.get_string(&ItemKey::InitialKey))
            .map(str::to_owned);
        {
            let tag = match bound.primary_tag_mut() {
                Some(tag) => tag,
                None => {
                    bound.insert_tag(Tag::new(TagType::Id3v2));
                    bound
                        .primary_tag_mut()
                        .expect("insert_tag 后主标签必然存在")
                }
            };
            if !bpm_trim.is_empty() {
                // ID3v2 的 TBPM 只接受整数；小数会被丢弃，因此统一取整写入。
                let bpm_text = if bpm_trim.parse::<u32>().is_ok() {
                    bpm_trim.to_owned()
                } else if let Ok(float_val) = bpm_trim.parse::<f64>() {
                    format!("{}", float_val.round() as u32)
                } else {
                    bpm_trim.to_owned() // 非数值文本原样尝试写入
                };
                tag.insert_text(ItemKey::IntegerBpm, bpm_text);
            }
            if !key_trim.is_empty() {
                tag.insert_text(ItemKey::InitialKey, key_trim.to_owned());
            }
        }
        bound
            .save(WriteOptions::default())
            .map_err(|e| format!("写回失败（文件未改动或部分改动）：{}", e))?;
        Ok(WriteResult {
            path: request.path,
            bpm_written: (!bpm_trim.is_empty()).then(|| bpm_trim.to_owned()),
            key_written: (!key_trim.is_empty()).then(|| key_trim.to_owned()),
            bpm_before,
            key_before,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct WaveformPeaks {
    /// 归一化到 0..1 的峰值序列，长度等于请求的 buckets。
    buckets: Vec<f32>,
    duration_sec: f64,
    sample_rate: u32,
    channels: u32,
    /// 解码器标识，供界面在失败时给出可读原因。
    codec: String,
}

/// 波形预览的时间分辨率：每 10 ms 记一个峰值，再按需压缩到固定槽数。
const WAVE_TILE_MS: u64 = 10;
/// 单个文件最多记录的峰值片数（约 600 秒@10ms），超长文件只截取前段。
const MAX_WAVE_TILES: usize = 60_000;

/// 解码整段音频，压成固定数量的归一化峰值用于画波形。
/// 只读文件、不写任何内容；解码失败的包跳过，不中断整条文件。
fn collect_peaks(path: &Path, buckets: usize) -> Result<WaveformPeaks, String> {
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
    let sample_rate = track.codec_params.sample_rate.unwrap_or(0);
    let codec = format!("{:?}", track.codec_params.codec);
    let codec_params = track.codec_params.clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(|e| format!("该格式暂不支持波形预览：{e}"))?;

    let buckets = buckets.clamp(16, 4096);
    let mut tiles: Vec<f32> = Vec::new();
    let mut tile_frames: usize = 0;
    let mut tile_cap: usize = 1;
    let mut acc: f32 = 0.0;
    let mut frames_total: u64 = 0;
    let mut channels: u32 = 0;
    let mut sample_buf: Option<SampleBuffer<f32>> = None;

    loop {
        if tiles.len() >= MAX_WAVE_TILES {
            break;
        }
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
            tile_cap = ((spec.rate as u64 * WAVE_TILE_MS / 1000).max(1)) as usize;
            channels = spec.channels.count() as u32;
            sample_buf = Some(SampleBuffer::<f32>::new(
                decoded.capacity() as u64,
                spec,
            ));
        }
        let buffer = sample_buf.as_mut().expect("样本缓冲已初始化");
        buffer.copy_interleaved_ref(decoded);
        let channel_count = spec.channels.count();
        let samples = buffer.samples();
        for frame in 0..frames {
            let base = frame * channel_count;
            let mut peak = 0f32;
            for ch in 0..channel_count {
                let value = samples[base + ch].abs();
                if value > peak {
                    peak = value;
                }
            }
            if peak > acc {
                acc = peak;
            }
            tile_frames += 1;
            if tile_frames >= tile_cap {
                tiles.push(acc);
                acc = 0.0;
                tile_frames = 0;
            }
        }
        frames_total += frames as u64;
    }

    if tile_frames > 0 {
        tiles.push(acc);
    }
    if tiles.is_empty() {
        return Err("没有解码出音频数据".to_string());
    }

    // 把 10ms 峰值片压缩到固定槽数：每槽取区间最大值，再按全曲峰值归一。
    let tile_len = tiles.len();
    let global = tiles.iter().copied().fold(0f32, f32::max);
    let mut out = Vec::with_capacity(buckets);
    for index in 0..buckets {
        let start = (index * tile_len / buckets).min(tile_len - 1);
        let end = (((index + 1) * tile_len) / buckets)
            .max(start + 1)
            .min(tile_len);
        let mut peak = 0f32;
        for slot in start..end {
            if tiles[slot] > peak {
                peak = tiles[slot];
            }
        }
        // 0.6 次幂抬升弱段，让安静的音乐段落也看得见。
        out.push(if global > 0.0 {
            (peak / global).powf(0.6)
        } else {
            0.0
        });
    }

    Ok(WaveformPeaks {
        buckets: out,
        duration_sec: if sample_rate > 0 {
            frames_total as f64 / sample_rate as f64
        } else {
            0.0
        },
        sample_rate,
        channels,
        codec,
    })
}

/// 波形预览：给界面画「试听」旁边的峰值条。只读，不解码失败就返回可读错误。
#[tauri::command]
async fn waveform_peaks(path: String, buckets: usize) -> Result<WaveformPeaks, String> {
    tauri::async_runtime::spawn_blocking(move || collect_peaks(Path::new(&path), buckets))
        .await
        .map_err(|e| e.to_string())?
}

/// 分析单个文件：默认引擎为本地 DSP。只读，不写任何标签。
#[tauri::command]
async fn analyze_file(path: String, engine: String) -> Result<analysis::AnalysisResult, String> {
    tauri::async_runtime::spawn_blocking(move || analysis::analyze_file(Path::new(&path), &engine))
        .await
        .map_err(|e| e.to_string())?
}

/// 拖放解析：拖入文件夹就用它；拖入音频文件就用它所在的文件夹。
#[tauri::command]
fn resolve_drop_folder(paths: Vec<String>) -> Option<String> {
    for raw in paths {
        let candidate = PathBuf::from(&raw);
        if candidate.is_dir() {
            return Some(candidate.to_string_lossy().into_owned());
        }
        if candidate.is_file() {
            if let Some(parent) = candidate.parent() {
                if parent.is_dir() {
                    return Some(parent.to_string_lossy().into_owned());
                }
            }
        }
    }
    None
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            app_info,
            pick_folder,
            scan_folder,
            write_metadata,
            waveform_peaks,
            analyze_file,
            resolve_drop_folder
        ])
        .run(tauri::generate_context!())
        .expect("启动 BPM Key Tools 失败");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 生成一个最小的合法 PCM WAV（44 字节头 + 4 字节静音），供读写往返测试。
    fn write_min_wav(path: &Path) {
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(b"RIFF");
        let payload = 4u32 + 16u32 + 8u32 + 8u32 + 88200u32; // WAVE + fmt + data 头 + 一秒静音
        data.extend_from_slice(&payload.to_le_bytes()); // 剩余块大小
        data.extend_from_slice(b"WAVE");
        data.extend_from_slice(b"fmt ");
        data.extend_from_slice(&16u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes()); // PCM
        data.extend_from_slice(&1u16.to_le_bytes()); // 单声道
        data.extend_from_slice(&44100u32.to_le_bytes());
        data.extend_from_slice(&88200u32.to_le_bytes()); // 字节率
        data.extend_from_slice(&2u16.to_le_bytes()); // 块对齐
        data.extend_from_slice(&16u16.to_le_bytes()); // 位深
        data.extend_from_slice(b"data");
        data.extend_from_slice(&88200u32.to_le_bytes());
        data.extend_from_slice(&vec![0u8; 88200]);
        std::fs::write(path, data).unwrap();
    }

    /// 生成 0.5 秒 440 Hz 正弦 PCM WAV，供波形峰值测试。
    fn write_tone_wav(path: &Path) {
        let rate = 44100u32;
        let frames = rate / 2;
        let samples: Vec<i16> = (0..frames)
            .map(|i| {
                let t = i as f32 / rate as f32;
                (i16::MAX as f32 * 0.5 * (2.0 * std::f32::consts::PI * 440.0 * t).sin()) as i16
            })
            .collect();
        let data_len = (samples.len() * 2) as u32;
        let mut data: Vec<u8> = Vec::new();
        data.extend_from_slice(b"RIFF");
        data.extend_from_slice(&(36 + data_len).to_le_bytes());
        data.extend_from_slice(b"WAVE");
        data.extend_from_slice(b"fmt ");
        data.extend_from_slice(&16u32.to_le_bytes());
        data.extend_from_slice(&1u16.to_le_bytes()); // PCM
        data.extend_from_slice(&1u16.to_le_bytes()); // 单声道
        data.extend_from_slice(&rate.to_le_bytes());
        data.extend_from_slice(&(rate * 2).to_le_bytes());
        data.extend_from_slice(&2u16.to_le_bytes());
        data.extend_from_slice(&16u16.to_le_bytes());
        data.extend_from_slice(b"data");
        data.extend_from_slice(&data_len.to_le_bytes());
        for sample in &samples {
            data.extend_from_slice(&sample.to_le_bytes());
        }
        std::fs::write(path, data).unwrap();
    }

    #[test]
    fn scan_then_write_roundtrip() {
        let dir = std::env::temp_dir().join("bpm-key-tool-test");
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("unit-roundtrip.wav");
        write_min_wav(&wav);

        // 扫描：应读到该文件，且初始无 BPM/调性标签。
        let scanned = read_track(&wav).expect("扫描应成功");
        assert!(scanned.bpm_existing.is_none());
        assert!(scanned.key_existing.is_none());
        assert!(scanned.duration_sec > 0.0);

        // 写入：BPM + 调性 → 旧值为空 → 回读应命中。
        let request = WriteRequest {
            path: wav.to_string_lossy().into_owned(),
            bpm: "120.0".into(),
            key: "A minor".into(),
        };
        let result = tauri::async_runtime::block_on(write_metadata(request)).expect("写入应成功");
        assert_eq!(result.bpm_written.as_deref(), Some("120.0"));
        assert_eq!(result.key_written.as_deref(), Some("A minor"));
        assert!(result.bpm_before.is_none());

        // ID3v2 的 TBPM 只存整数：请求 120.0 → 落盘并回读为 120。
        let reread = read_track(&wav).expect("回读应成功");
        assert_eq!(reread.bpm_existing.as_deref(), Some("120"));
        assert_eq!(reread.key_existing.as_deref(), Some("A minor"));
        std::fs::remove_file(&wav).ok();
    }

    #[test]
    fn waveform_peaks_from_wav() {
        let dir = std::env::temp_dir().join("bpm-key-tool-test");
        std::fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("unit-wave.wav");
        write_tone_wav(&wav);

        let peaks = collect_peaks(&wav, 64).expect("波形应可解析");
        assert_eq!(peaks.buckets.len(), 64);
        assert_eq!(peaks.channels, 1);
        assert!(
            (peaks.duration_sec - 0.5).abs() < 0.05,
            "时长应约 0.5s，实际 {}",
            peaks.duration_sec
        );
        let max = peaks.buckets.iter().copied().fold(0f32, f32::max);
        assert!(max > 0.99, "归一后峰值应接近 1，实际 {max}");
        std::fs::remove_file(&wav).ok();
    }

    /// 手动验证：真实文件的波形解码（AIFF / FLAC / MP3 / M4A / WAV）。
    /// 用法：$env:BPM_WAVE_SAMPLE_DIR="<目录>"; cargo test waveform_real_files -- --ignored --nocapture
    #[test]
    #[ignore]
    fn waveform_real_files() {
        let Ok(dir) = std::env::var("BPM_WAVE_SAMPLE_DIR") else {
            return;
        };
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if !is_audio(&path) {
                continue;
            }
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            match collect_peaks(&path, 480) {
                Ok(res) => println!(
                    "{name} → {:.2}s / {} Hz / {} ch / {} 槽 / codec={}",
                    res.duration_sec,
                    res.sample_rate,
                    res.channels,
                    res.buckets.len(),
                    res.codec
                ),
                Err(err) => println!("{name} → 失败：{err}"),
            }
        }
    }

    #[test]
    fn diag_tag_frames() {
        let mut tag = Tag::new(TagType::Id3v2);
        tag.insert_text(ItemKey::IntegerBpm, "120.0".to_owned());
        tag.insert_text(ItemKey::InitialKey, "A minor".to_owned());
        println!(
            "bpm={:?} key={:?} frame_count={}",
            tag.get_string(&ItemKey::IntegerBpm),
            tag.get_string(&ItemKey::InitialKey),
            tag.len()
        );
        assert_eq!(tag.get_string(&ItemKey::IntegerBpm), Some("120.0"));
    }
}
