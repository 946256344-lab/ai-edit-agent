//! 发行就绪检查：启动时探测媒体 runtime、本地目录、模型凭据与剪映交付前置，不改业务副作用。
use crate::custom_api::get_custom_api_status;
use crate::db::database_path;
use crate::oauth::get_experimental_openai_oauth_status;
use crate::process::{
    hidden_command, program_responds, python_program, run_hidden_command_with_timeout,
    tesseract_program,
};
use crate::storyboard::semantic;
use serde::Serialize;
use std::{collections::BTreeMap, fs, path::Path, time::Duration};
use tauri::{AppHandle, Manager};

const MIN_FREE_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReleaseReadinessCheck {
    pub id: String,
    pub title: String,
    pub status: String,
    pub message: String,
    /// 稳定文案键：前端按界面语言翻译；中文 title/message 保留作回落与日志。
    pub message_key: String,
    pub message_params: BTreeMap<String, String>,
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
        message_key: String::new(),
        message_params: BTreeMap::new(),
    }
}

impl ReleaseReadinessCheck {
    fn key(mut self, key: &str) -> Self {
        self.message_key = key.to_owned();
        self
    }

    fn param(mut self, name: &str, value: impl Into<String>) -> Self {
        self.message_params.insert(name.to_owned(), value.into());
        self
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
    let mut tesseract = hidden_command(tesseract_program());
    tesseract.arg("--list-langs");
    let tesseract_ok = run_hidden_command_with_timeout(&mut tesseract, Duration::from_secs(8))
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line.trim() == "eng")
        })
        .unwrap_or(false);
    vec![
        if ffmpeg_ok {
            check("ffmpeg", "媒体处理", "ok", "FFmpeg 可用。").key("ffmpeg.ok")
        } else {
            check(
                "ffmpeg",
                "媒体处理",
                "fail",
                "未找到 FFmpeg。正式安装包应已包含；请重新安装应用后再试。",
            )
            .key("ffmpeg.missing")
        },
        if ffprobe_ok {
            check("ffprobe", "媒体探测", "ok", "FFprobe 可用。").key("ffprobe.ok")
        } else {
            check(
                "ffprobe",
                "媒体探测",
                "fail",
                "未找到 FFprobe。正式安装包应已包含；请重新安装应用后再试。",
            )
            .key("ffprobe.missing")
        },
        if tesseract_ok {
            check("tesseract", "文字识别", "ok", "Tesseract 英文 OCR 可用。").key("tesseract.ok")
        } else {
            check(
                "tesseract",
                "文字识别",
                "fail",
                "未找到 Tesseract 或英文 OCR 数据。正式安装包应已包含；请重新安装应用后再试。",
            )
            .key("tesseract.missing")
        },
    ]
}

fn provider_check() -> ReleaseReadinessCheck {
    match crate::fellowcut_account::gateway_base_url() {
        Ok(Some(_)) => {
            return check(
                "provider",
                "AI 模型",
                "ok",
                "Voycut 模型服务已配置；使用资格在请求时校验。",
            );
        }
        Err(_) => {
            return check(
                "provider",
                "AI 模型",
                "warn",
                "Voycut 模型服务地址无效或未配置。",
            );
        }
        Ok(None) => {}
    }
    let custom = get_custom_api_status();
    if custom.state == "connected" {
        return check("provider", "AI 模型", "ok", "自定义 API 已连接。")
            .key("provider.customConnected");
    }
    let oauth = get_experimental_openai_oauth_status();
    if oauth.state == "connected" {
        return check("provider", "AI 模型", "ok", "ChatGPT 登录已连接。")
            .key("provider.oauthConnected");
    }
    if custom.state == "failed" || oauth.state == "failed" {
        return check(
            "provider",
            "AI 模型",
            "warn",
            "模型凭据读取异常。请打开设置重新连接，凭据不会写入日志。",
        )
        .key("provider.credentialError");
    }
    check(
        "provider",
        "AI 模型",
        "warn",
        "尚未连接 AI 模型。导入素材可以继续，开始剪辑前请先在设置里连接。",
    )
    .key("provider.notConnected")
}

fn jianying_checks(app: &AppHandle) -> Vec<ReleaseReadinessCheck> {
    let mut checks = Vec::new();
    checks.push(if crate::jianying::draft_location_available() {
        check("jianying", "剪映草稿位置", "ok", "已找到剪映草稿目录。").key("jianying.ok")
    } else {
        check(
            "jianying",
            "剪映草稿位置",
            "warn",
            "未找到剪映草稿目录。仍可预览；打开剪映前请先安装并启动过剪映。",
        )
        .key("jianying.missing")
    });

    let script_ok = crate::jianying::adapter_script_available(app);
    let python = python_program();
    let python_ok = program_responds(&python, &["--version"], Duration::from_secs(8));
    let sdk_ok = python_ok
        && program_responds(
            &python,
            &["-c", "import pyJianYingDraft, pycapcut"],
            Duration::from_secs(20),
        );
    checks.push(if script_ok && sdk_ok {
        check(
            "jianying_adapter",
            "剪映适配器",
            "ok",
            "随包 Python 与剪映草稿 SDK 可用。",
        )
        .key("jianyingAdapter.ok")
    } else if !script_ok {
        check(
            "jianying_adapter",
            "剪映适配器",
            "warn",
            "缺少剪映草稿脚本资源。预览不受影响，无法创建剪映草稿。",
        )
        .key("jianyingAdapter.scriptMissing")
    } else if python_ok {
        check(
            "jianying_adapter",
            "剪映适配器",
            "warn",
            "Python 可用，但未找到 pyJianYingDraft/pycapcut。预览不受影响，无法创建剪映草稿。",
        )
        .key("jianyingAdapter.sdkMissing")
    } else {
        check(
            "jianying_adapter",
            "剪映适配器",
            "warn",
            "未找到随包 Python。预览不受影响，无法创建剪映草稿。",
        )
        .key("jianyingAdapter.pythonMissing")
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
        )
        .key("semanticModel.ok"),
        Ok(false) | Err(_) => {
            let downloading = crate::runtime_models::get_runtime_model_status(app.clone())
                .ok()
                .map(|status| matches!(status.overall.as_str(), "downloading" | "pending"))
                .unwrap_or(false);
            if downloading {
                check(
                    "semantic_model",
                    "本地语义模型",
                    "warn",
                    "正在下载本地语义模型。选镜仍可进行，完成前会降级为词面匹配。",
                )
                .key("semanticModel.downloading")
            } else {
                check(
                    "semantic_model",
                    "本地语义模型",
                    "warn",
                    "本地语义模型资源缺失。选镜仍可进行，但会降级为词面匹配。可在提醒条中重试下载。",
                )
        .key("semanticModel.missing")
            }
        }
    }
}

fn clip_model_check(app: &AppHandle) -> ReleaseReadinessCheck {
    match crate::storyboard::clip::bundled_models_present(app) {
        Ok(true) => check(
            "clip_model",
            "本地 CLIP 模型",
            "ok",
            &format!(
                "CLIP 图文模型可用（{} / {}）。",
                crate::storyboard::clip::CLIP_VISION_MODEL,
                crate::storyboard::clip::CLIP_TEXT_MODEL
            ),
        )
        .key("clipModel.ok")
        .param("vision", crate::storyboard::clip::CLIP_VISION_MODEL)
        .param("text", crate::storyboard::clip::CLIP_TEXT_MODEL),
        Ok(false) | Err(_) => {
            let downloading = crate::runtime_models::get_runtime_model_status(app.clone())
                .ok()
                .map(|status| matches!(status.overall.as_str(), "downloading" | "pending"))
                .unwrap_or(false);
            if downloading {
                check(
                    "clip_model",
                    "本地 CLIP 模型",
                    "warn",
                    "正在下载 CLIP 图文模型。选镜仍可进行，完成前不会用画面向量加权。",
                )
                .key("clipModel.downloading")
            } else {
                check(
                    "clip_model",
                    "本地 CLIP 模型",
                    "warn",
                    "CLIP 图文模型资源缺失。选镜仍可进行，但不会用画面向量加权。可在提醒条中重试下载。",
                )
        .key("clipModel.missing")
            }
        }
    }
}

#[tauri::command(async)]
pub fn get_release_readiness(app: AppHandle) -> Result<ReleaseReadinessReport, String> {
    let mut checks = Vec::new();
    checks.extend(media_runtime_checks());

    checks.push(match data_directory_writable(&app) {
        Ok(()) => check("data_dir", "本地数据目录", "ok", "应用数据目录可写。").key("dataDir.ok"),
        Err(_) => check(
            "data_dir",
            "本地数据目录",
            "fail",
            "本地数据目录不可写。请检查磁盘权限或更换用户数据目录。",
        )
        .key("dataDir.notWritable"),
    });

    if let Ok(directory) = app.path().app_data_dir() {
        match free_bytes_for_path(&directory) {
            Some(bytes) if bytes >= MIN_FREE_BYTES => checks.push(
                check(
                    "disk_space",
                    "磁盘空间",
                    "ok",
                    format!("可用空间约 {}。", format_gib(bytes)),
                )
                .key("diskSpace.ok")
                .param("size", format_gib(bytes)),
            ),
            Some(bytes) => checks.push(
                check(
                    "disk_space",
                    "磁盘空间",
                    "warn",
                    format!(
                        "可用空间约 {}，建议至少保留 2 GB 给预览与分析缓存。",
                        format_gib(bytes)
                    ),
                )
                .key("diskSpace.low")
                .param("size", format_gib(bytes)),
            ),
            None => checks.push(
                check(
                    "disk_space",
                    "磁盘空间",
                    "warn",
                    "无法读取剩余磁盘空间，请确保系统盘有足够空闲。",
                )
                .key("diskSpace.unknown"),
            ),
        }
    }

    checks.push(provider_check());
    checks.extend(jianying_checks(&app));
    checks.push(if crate::capcut::draft_location_available() {
        check(
            "capcut",
            "CapCut 草稿位置",
            "ok",
            "已找到本机 CapCut 草稿目录。",
        )
        .key("capcut.ok")
    } else {
        check(
            "capcut",
            "CapCut 草稿位置",
            "warn",
            "未找到 CapCut 草稿目录。仍可预览；换设备后请先打开一次 CapCut 并创建本地草稿。",
        )
        .key("capcut.missing")
    });
    checks.push(semantic_model_check(&app));
    checks.push(clip_model_check(&app));

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
