#![windows_subsystem = "windows"]

/// 供前端查询应用与算法状态。算法内核尚未接入，这里如实返回 not_connected。
#[tauri::command]
fn app_info() -> serde_json::Value {
    serde_json::json!({
        "name": "BPM 调性识别",
        "version": env!("CARGO_PKG_VERSION"),
        "algorithm": "not_connected",
        "mock": true,
        "stage": "S2-shell"
    })
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![app_info])
        .run(tauri::generate_context!())
        .expect("启动 BPM 调性识别失败");
}
