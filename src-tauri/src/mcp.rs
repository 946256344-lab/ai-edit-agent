//! 本地 stdio MCP 服务：外部 Agent 查已分析素材，按镜头清单新建时间线并交付编辑器草稿。
//!
//! 与桌面应用共用同一 SQLite（WAL）；不暴露凭据，不触发会花模型额度的分析。
//! 日志只写 stderr，stdout 专供 JSON-RPC。
//! 桌面命令 `get_mcp_install_info` 只解析本机 sidecar 路径，不启动 MCP 进程。

mod protocol;
mod tools;

use serde::Serialize;
use std::path::{Path, PathBuf};
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

/// 本机 `voycut-mcp` 路径与可复制的 Claude Code / Codex 接入片段。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpInstallInfo {
    pub executable_path: Option<String>,
    pub exists: bool,
    pub claude_command: String,
    pub codex_toml: String,
}

#[tauri::command(async)]
pub async fn get_mcp_install_info() -> Result<McpInstallInfo, String> {
    Ok(resolve_mcp_install_info())
}

fn mcp_binary_name() -> &'static str {
    if cfg!(windows) {
        "voycut-mcp.exe"
    } else {
        "voycut-mcp"
    }
}

fn fallback_install_path() -> PathBuf {
    if cfg!(windows) {
        PathBuf::from(r"C:\Program Files\Voycut").join(mcp_binary_name())
    } else {
        PathBuf::from("/usr/local/bin").join(mcp_binary_name())
    }
}

fn mcp_executable_candidates() -> Vec<PathBuf> {
    let file_name = mcp_binary_name();
    let mut paths = Vec::new();
    for dir in crate::process::exe_ancestor_dirs() {
        paths.push(dir.join(file_name));
    }
    #[cfg(debug_assertions)]
    {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        paths.push(manifest.join("target").join("debug").join(file_name));
        paths.push(
            manifest
                .join("target")
                .join("release")
                .join(file_name),
        );
    }
    paths
}

fn resolve_mcp_install_info() -> McpInstallInfo {
    if let Some(path) = mcp_executable_candidates()
        .into_iter()
        .find(|path| path.is_file())
    {
        return mcp_install_info_from_path(&path, true);
    }
    let expected = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|parent| parent.join(mcp_binary_name())))
        .unwrap_or_else(fallback_install_path);
    mcp_install_info_from_path(&expected, false)
}

fn mcp_install_info_from_path(path: &Path, exists: bool) -> McpInstallInfo {
    let display = path.display().to_string();
    let escaped = display.replace('"', "\\\"");
    let toml_path = display.replace('\\', "\\\\").replace('"', "\\\"");
    McpInstallInfo {
        executable_path: Some(display),
        exists,
        claude_command: format!("claude mcp add voycut -- \"{escaped}\""),
        codex_toml: format!("[mcp_servers.voycut]\ncommand = \"{toml_path}\""),
    }
}

#[cfg(test)]
mod tests {
    use super::{mcp_install_info_from_path, McpInstallInfo};
    use std::path::Path;

    #[test]
    fn quotes_windows_path_for_claude_and_codex() {
        let info = mcp_install_info_from_path(
            Path::new(r"C:\Program Files\Voycut\voycut-mcp.exe"),
            true,
        );
        assert_eq!(
            info,
            McpInstallInfo {
                executable_path: Some(r"C:\Program Files\Voycut\voycut-mcp.exe".to_owned()),
                exists: true,
                claude_command: r#"claude mcp add voycut -- "C:\Program Files\Voycut\voycut-mcp.exe""#
                    .to_owned(),
                codex_toml: "[mcp_servers.voycut]\ncommand = \"C:\\\\Program Files\\\\Voycut\\\\voycut-mcp.exe\""
                    .to_owned(),
            }
        );
    }
}
