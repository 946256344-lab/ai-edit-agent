//! 本地 stdio MCP 服务：外部 Agent 查已分析素材，按镜头清单新建时间线并交付编辑器草稿。
//!
//! 与桌面应用共用同一 SQLite（WAL）；不暴露凭据，不触发会花模型额度的分析。
//! 日志只写 stderr，stdout 专供 JSON-RPC。

mod protocol;
mod tools;

use std::path::PathBuf;
use tauri::Manager;

pub fn run() -> Result<(), String> {
    let app = build_headless_app()?;
    let handle = app.handle().clone();
    crate::process::install_bundled_media_tools(&handle);
    protocol::serve(handle)
}

fn build_headless_app() -> Result<tauri::App, String> {
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    if let Some(data) = override_data_dir() {
        context.config_mut().identifier = data.to_string_lossy().into_owned();
    }
    let app = tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::default()
                .level(log::LevelFilter::Info)
                .targets([tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stderr,
                )])
                .build(),
        )
        .build(context)
        .map_err(|error| error.to_string())?;
    if !app.handle().webview_windows().is_empty() {
        return Err("mcp_window_created".to_owned());
    }
    Ok(app)
}

fn override_data_dir() -> Option<PathBuf> {
    let raw = std::env::var_os("VOYCUT_APP_DATA_DIR")?;
    if raw.is_empty() {
        return None;
    }
    let path = PathBuf::from(raw);
    std::fs::create_dir_all(&path).ok()?;
    path.canonicalize().ok().or(Some(path))
}
