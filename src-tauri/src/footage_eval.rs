//! 评测专用入口与观测辅助；独立数据根、真实 Agent 链路，默认构建不包含。
use serde_json::{json, Value};
use std::{fs, path::PathBuf, sync::OnceLock};
use tauri::Manager;

static OUTPUT: OnceLock<PathBuf> = OnceLock::new();

pub(crate) fn trace_directory() -> Option<PathBuf> {
    OUTPUT.get().cloned()
}

pub(crate) fn redact(value: &Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(map.iter().map(|(key, value)| {
            let lower = key.to_ascii_lowercase();
            let hidden = ["authorization", "api_key", "apikey", "token", "password", "secret", "image_url"]
                .iter().any(|needle| lower.contains(needle));
            (key.clone(), if hidden { json!("[omitted]") } else { redact(value) })
        }).collect()),
        Value::Array(items) => Value::Array(items.iter().map(redact).collect()),
        Value::String(text) => {
            if let Ok(nested) = serde_json::from_str::<Value>(text) {
                json!(redact(&nested).to_string())
            } else { value.clone() }
        }
        _ => value.clone(),
    }
}

pub(crate) fn redact_body(body: &str) -> String {
    serde_json::from_str::<Value>(body).map(|value| redact(&value).to_string())
        .unwrap_or_else(|_| "[non-JSON body omitted]".to_owned())
}

/// 评测只读系统凭据，不刷新 OAuth / 登录令牌，不改变 Provider。
pub(crate) fn model_access() -> Result<crate::provider::ModelAccess, String> {
    if crate::fellowcut_account::gateway_base_url()?.is_some() {
        return Err("eval_gateway_requires_mutable_login_credentials".to_owned());
    }
    let config = crate::custom_api::custom_config()?
        .ok_or_else(|| "eval_custom_provider_not_configured_oauth_refresh_not_allowed".to_owned())?;
    let metadata = json!({"provider": "custom", "baseUrl": config.base_url,
        "model": config.model, "coarseVisualModel": config.coarse_visual_model});
    if let Some(directory) = trace_directory() {
        fs::write(directory.join("provider.json"), metadata.to_string()).map_err(|e| e.to_string())?;
    }
    Ok(crate::provider::ModelAccess::Custom(config))
}

pub fn run() -> Result<(), String> {
    let input = std::env::args_os().nth(1).ok_or("missing job.json")?;
    let input = fs::canonicalize(input).map_err(|e| e.to_string())?;
    let directory = input.parent().ok_or("missing output directory")?.to_path_buf();
    OUTPUT.set(directory.clone()).map_err(|_| "already initialized")?;
    let job: Value = serde_json::from_slice(&fs::read(input).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    std::env::set_var("STORYBOARD_PROVIDER_TRACE", "1");
    std::env::set_var("NATIVE_PROVIDER_FULL_TRACE", "1");
    let temp = directory.join("temp");
    std::env::set_var("TEMP", &temp);
    std::env::set_var("TMP", &temp);
    let mut context = tauri::generate_context!();
    let original_identifier = context.config().identifier.clone();
    context.config_mut().app.windows.clear();
    // Tauri 的所有 app_* 路径都是系统目录 join identifier；绝对 identifier 将它们
    // 统一指向评测根。创建窗口前已清空配置，并在使用数据库前逐项断言。
    let data = directory.join("appdata");
    context.config_mut().identifier = data.to_string_lossy().into_owned();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().level(log::LevelFilter::Info)
            .targets([tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout)])
            .build())
        .build(context).map_err(|e| e.to_string())?;
    let handle = app.handle();
    for path in [handle.path().app_data_dir(), handle.path().app_local_data_dir(),
        handle.path().app_cache_dir(), handle.path().app_config_dir(), handle.path().app_log_dir()] {
        if !path.map_err(|e| e.to_string())?.starts_with(&data) {
            return Err("eval_path_isolation_failed".to_owned());
        }
    }
    let original_data = handle.path().data_dir().map_err(|e| e.to_string())?.join(original_identifier);
    if original_data.is_dir() {
        let original_data = fs::canonicalize(original_data).map_err(|e| e.to_string())?;
        let actual_data = fs::canonicalize(&data).map_err(|e| e.to_string())?;
        if actual_data.starts_with(original_data) {
            return Err("eval_real_app_data_directory_forbidden".to_owned());
        }
    }
    if !handle.webview_windows().is_empty() { return Err("eval_window_created".to_owned()); }
    fs::create_dir_all(&temp).map_err(|e| e.to_string())?;
    crate::process::install_bundled_media_tools(handle);
    let text = |key: &str| job[key].as_str().unwrap_or_default().to_owned();
    let outcome = crate::agent::run_agent_edit_pipeline(handle.clone(), &text("agentTaskId"),
        text("projectId"), text("editingTaskId"), text("conversationId"),
        job["storyboardVersionId"].as_str().map(str::to_owned),
        job["timelineVersionId"].as_str().map(str::to_owned), text("request"));
    let result = match outcome {
        Ok(result) => json!({"result": result,
            "status": crate::agent::persisted_task_status(handle, &text("agentTaskId")),
            "isolation": {"dataDirectory": data, "windows": 0}, "modelEvaluation": "live"}),
        Err(error) => json!({"status": "runner_failed", "error": error,
            "isolation": {"dataDirectory": data, "windows": 0}, "modelEvaluation": "live_attempt"}),
    };
    fs::write(directory.join("result.json"), serde_json::to_vec_pretty(&redact(&result)).unwrap())
        .map_err(|e| e.to_string())?;
    Ok(())
}
