//! 发行就绪检查：启动时探测媒体 runtime、本地目录、模型凭据与剪映交付前置，不改业务副作用。
use crate::custom_api::get_custom_api_status;
use crate::db::database_path;
use crate::oauth::get_experimental_openai_oauth_status;
use crate::process::program_responds;
use crate::storyboard::semantic;
use serde::Serialize;
use std::{fs, path::Path, time::Duration};
use tauri::{AppHandle, Manager};

const MIN_FREE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseReadinessCheck {
    pub id: String,
    pub title: String,
    pub status: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseReadinessReport {
    pub overall: String,
    pub checks: Vec<ReleaseReadinessCheck>,
}

fn check(id: &str, title: &str, status: &str, message: impl Into<String>) -> ReleaseReadinessCheck {
    ReleaseReadinessCheck {
        id: id.to_owned(),
        title: title.to_owned(),
        status: status.to_owned(),
        message: message.into(),
    }
}

fn tool_available(program: &str, args: &[&str]) -> bool {
    program_responds(program, args, Duration::from_secs(5))
}

fn data_directory_writable(app: &AppHandle) -> Result<(), String> {
    let directory = app
        .path()
        .app_data_dir()
        .map_err(|error| error.to_string())?;
    fs::create_dir_all(&directory).map_err(|error| error.to_string())?;
    let probe = directory.join(".release-readiness-write-probe");
    fs::write(&probe, b"ok").map_err(|error| error.to_string())?;
    let _ = fs::remove_file(&probe);
    let _ = database_path(app)?;
    Ok(())
}

#[cfg(windows)]
fn free_bytes_for_path(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetDiskFreeSpaceExW(
            lp_directory_name: *const u16,
            lp_free_bytes_available_to_caller: *mut u64,
            lp_total_number_of_bytes: *mut u64,
            lp_total_number_of_free_bytes: *mut u64,
        ) -> i32;
    }
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut available = 0_u64;
    let mut total = 0_u64;
    let mut free = 0_u64;
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free) };
    if ok != 0 {
        Some(available)
    } else {
        None
    }
}

#[cfg(not(windows))]
fn free_bytes_for_path(_path: &Path) -> Option<u64> {
    None
}

fn format_gib(bytes: u64) -> String {
    format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
}

fn media_runtime_checks() -> Vec<ReleaseReadinessCheck> {
    let ffmpeg_ok = tool_available("ffmpeg", &["-version"]);
    let ffprobe_ok = tool_available("ffprobe", &["-version"]);
    vec![
        if ffmpeg_ok {
            check("ffmpeg", "媒体处理", "ok", "FFmpeg 可用。")
        } else {
            check(
                "ffmpeg",
                "媒体处理",
                "fail",
                "未找到 FFmpeg。导入分析与预览都需要它，请安装并确保可在本机命令行运行 ffmpeg。",
            )
        },
        if ffprobe_ok {
            check("ffprobe", "媒体探测", "ok", "FFprobe 可用。")
        } else {
            check(
                "ffprobe",
                "媒体探测",
                "fail",
                "未找到 FFprobe。素材时长与格式探测需要它。",
            )
        },
    ]
}

fn provider_check() -> ReleaseReadinessCheck {
    let custom = get_custom_api_status();
    if custom.state == "connected" {
        return check("provider", "AI 模型", "ok", "自定义 API 已连接。");
    }
    let oauth = get_experimental_openai_oauth_status();
    if oauth.state == "connected" {
        return check("provider", "AI 模型", "ok", "ChatGPT 登录已连接。");
    }
    if custom.state == "failed" || oauth.state == "failed" {
        return check(
            "provider",
            "AI 模型",
            "warn",
            "模型凭据读取异常。请打开设置重新连接，凭据不会写入日志。",
        );
    }
    check(
        "provider",
        "AI 模型",
        "warn",
        "尚未连接 AI 模型。导入素材可以继续，开始剪辑前请先在设置里连接。",
    )
}

fn jianying_checks(app: &AppHandle) -> Vec<ReleaseReadinessCheck> {
    let mut checks = Vec::new();
    checks.push(if crate::jianying::draft_location_available() {
        check("jianying", "剪映草稿位置", "ok", "已找到剪映草稿目录。")
    } else {
        check(
            "jianying",
            "剪映草稿位置",
            "warn",
            "未找到剪映草稿目录。仍可预览；打开剪映前请先安装并启动过剪映。",
        )
    });

    let script_ok = crate::jianying::adapter_script_available(app);
    let python_ok =
        tool_available("py", &["-3", "--version"]) || tool_available("python", &["--version"]);
    checks.push(if script_ok && python_ok {
        check(
            "jianying_adapter",
            "剪映适配器",
            "ok",
            "Python 与剪映草稿脚本可用。",
        )
    } else if !script_ok {
        check(
            "jianying_adapter",
            "剪映适配器",
            "warn",
            "缺少剪映草稿脚本资源。预览不受影响，无法创建剪映草稿。",
        )
    } else {
        check(
            "jianying_adapter",
            "剪映适配器",
            "warn",
            "未找到 Python（py/python）。预览不受影响，创建剪映草稿需要本机 Python。",
        )
    });
    checks
}

fn semantic_model_check(app: &AppHandle) -> ReleaseReadinessCheck {
    match semantic::bundled_model_present(app) {
        Ok(true) => check(
            "semantic_model",
            "本地语义模型",
            "ok",
            "本地语义模型资源可用。",
        ),
        Ok(false) | Err(_) => check(
            "semantic_model",
            "本地语义模型",
            "warn",
            "本地语义模型资源缺失。选镜仍可进行，但会降级为词面匹配。",
        ),
    }
}

#[tauri::command]
pub fn get_release_readiness(app: AppHandle) -> Result<ReleaseReadinessReport, String> {
    let mut checks = Vec::new();
    checks.extend(media_runtime_checks());

    checks.push(match data_directory_writable(&app) {
        Ok(()) => check("data_dir", "本地数据目录", "ok", "应用数据目录可写。"),
        Err(_) => check(
            "data_dir",
            "本地数据目录",
            "fail",
            "本地数据目录不可写。请检查磁盘权限或更换用户数据目录。",
        ),
    });

    if let Ok(directory) = app.path().app_data_dir() {
        match free_bytes_for_path(&directory) {
            Some(bytes) if bytes >= MIN_FREE_BYTES => checks.push(check(
                "disk_space",
                "磁盘空间",
                "ok",
                format!("可用空间约 {}。", format_gib(bytes)),
            )),
            Some(bytes) => checks.push(check(
                "disk_space",
                "磁盘空间",
                "warn",
                format!(
                    "可用空间约 {}，建议至少保留 2 GB 给预览与分析缓存。",
                    format_gib(bytes)
                ),
            )),
            None => checks.push(check(
                "disk_space",
                "磁盘空间",
                "warn",
                "无法读取剩余磁盘空间，请确保系统盘有足够空闲。",
            )),
        }
    }

    checks.push(provider_check());
    checks.extend(jianying_checks(&app));
    checks.push(semantic_model_check(&app));

    let overall = if checks.iter().any(|item| item.status == "fail") {
        "blocked"
    } else if checks.iter().any(|item| item.status == "warn") {
        "degraded"
    } else {
        "ready"
    };

    Ok(ReleaseReadinessReport {
        overall: overall.to_owned(),
        checks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overall_prefers_fail_over_warn() {
        let checks = vec![
            check("a", "A", "ok", "ok"),
            check("b", "B", "warn", "warn"),
            check("c", "C", "fail", "fail"),
        ];
        let overall = if checks.iter().any(|item| item.status == "fail") {
            "blocked"
        } else if checks.iter().any(|item| item.status == "warn") {
            "degraded"
        } else {
            "ready"
        };
        assert_eq!(overall, "blocked");
    }
}
