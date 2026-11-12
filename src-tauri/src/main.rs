#![windows_subsystem = "windows"]

use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::BoundTaggedFile;
use lofty::prelude::*;
use lofty::probe::Probe;
use lofty::tag::{Tag, TagType};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// 供前端查询应用与算法状态。BPM/调性识别算法仍未接入（值来自既有标签或人工判定），
/// 但扫描、试听、元数据写入已接真实文件。
#[tauri::command]
fn app_info() -> serde_json::Value {
    serde_json::json!({
        "name": "BPM Key Tools",
        "version": env!("CARGO_PKG_VERSION"),
        "algorithm": "not_connected",
        "mock": false,
        "stage": "S3-real-audit",
        "capabilities": ["scan", "playback", "metadata_write"]
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

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            app_info,
            pick_folder,
            scan_folder,
            write_metadata
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
