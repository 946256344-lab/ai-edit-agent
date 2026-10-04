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
    if std::env::args().nth(1).as_deref() == Some("--export-evidence") {
        return export_segment_evidence();
    }
    if std::env::args().nth(1).as_deref() == Some("--validate-baseline") {
        return validate_saved_storyboards();
    }
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
    if job["mode"].as_str() == Some("verify-evidence") {
        let candidates: Vec<crate::models::EvidenceVerificationRequest> = serde_json::from_value(job["candidates"].clone()).map_err(|e|e.to_string())?;
        let results = crate::assets::evidence_verification::verify_candidates(handle,job["projectId"].as_str().ok_or("missing project")?,&candidates)?;
        fs::write(directory.join("verification-result.json"),serde_json::to_vec_pretty(&json!({"results":results,"isolation":{"dataDirectory":data,"windows":0},"modelEvaluation":"live_candidate_verification"})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        return Ok(());
    }
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

/// 只读冻结数据库或运行副本；无 Tauri、无迁移、无 Provider、无媒体写入。
fn export_segment_evidence() -> Result<(), String> {
    let database = std::env::args_os().nth(2).ok_or("missing database")?;
    let output = std::env::args_os().nth(3).ok_or("missing evidence output")?;
    let connection = rusqlite::Connection::open_with_flags(database,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
    let has_verifications: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='asset_evidence_verifications')", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    let mut statement = connection.prepare("SELECT id,metadata_json FROM assets WHERE kind='video' AND coalesce(json_extract(metadata_json,'$.libraryRemoved'),0)=0 ORDER BY id").map_err(|e| e.to_string())?;
    let assets = statement.query_map([], |row| Ok((row.get::<_,String>(0)?, row.get::<_,String>(1)?))).map_err(|e| e.to_string())?;
    let mut evidence = Vec::new();
    for asset in assets {
        let (id, raw) = asset.map_err(|e| e.to_string())?;
        let metadata: crate::models::TechnicalMetadata = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
        for segment in crate::assets::evidence_contract::adapt_asset(&id, &metadata) {
            evidence.push(if has_verifications {
                crate::assets::evidence_contract::with_verifications(&connection, segment)?
            } else { segment });
        }
    }
    fs::write(output, serde_json::to_vec_pretty(&evidence).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

/// 保存的产物在冻结素材范围上重跑现行 P5；这是规则回放，不冒充模型重新生成。
fn validate_saved_storyboards() -> Result<(), String> {
    let database = std::env::args_os().nth(2).ok_or("missing database")?;
    let input = std::env::args_os().nth(3).ok_or("missing evidence input")?;
    let output = std::env::args_os().nth(4).ok_or("missing validation output")?;
    let connection = rusqlite::Connection::open_with_flags(database, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).map_err(|e| e.to_string())?;
    let mut statement = connection.prepare("SELECT id,kind,metadata_json FROM assets WHERE kind='video' AND coalesce(json_extract(metadata_json,'$.libraryRemoved'),0)=0 ORDER BY id").map_err(|e|e.to_string())?;
    let rows = statement.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?))).map_err(|e|e.to_string())?;
    let mut sources = Vec::new();
    for row in rows {
        let (asset_id, kind, raw) = row.map_err(|e|e.to_string())?;
        let m: crate::models::TechnicalMetadata = serde_json::from_str(&raw).map_err(|e|e.to_string())?;
        sources.push(crate::models::StoryboardSource { asset_id, kind, duration_ms:m.duration_ms,
            scene_segments:m.scene_segments, ocr_evidence:m.ocr_evidence, visual_evidence:m.visual_evidence,
            visual_quality_score:m.visual_quality_score, evidence_embedding:None, keyframe_grid_path:None,
            keyframes:m.keyframes, source_path:None, segment:None, segment_embedding:None, segment_clip_embedding:None });
    }
    let evidence: Value = serde_json::from_slice(&fs::read(input).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
    let mut results = Vec::new();
    for row in evidence["storyboard_versions"].as_array().ok_or("missing storyboard versions")? {
        let content: crate::models::StoryboardContent = serde_json::from_str(row["content_json"].as_str().ok_or("missing content")?).map_err(|e|e.to_string())?;
        let outcome = crate::storyboard::validate_storyboard(&content,&sources,&content.brief);
        results.push(json!({"id":row["id"],"passed":outcome.is_ok(),"error":outcome.err(),"shots":content.shots.len()}));
    }
    fs::write(output,serde_json::to_vec_pretty(&json!({"track":"saved_output_rule_replay","versions":results})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())
}
