//! 分析失败自动补跑：瞬时网络/超时可再入队，用户跳过与不适用不补。
//! 最后不足 6 段也照样送审，不攒满再发。
use rusqlite::{params, OptionalExtension};
use std::collections::HashMap;
use tauri::AppHandle;

use crate::db::{now_millis, open_connection};
use crate::models::TechnicalMetadata;

use super::visual::{queue_visual_analysis_batch, VISUAL_ANALYSIS_BATCH_SIZE};

pub(super) const VISUAL_AUTO_RETRY_LIMIT: u32 = 3;
pub(super) const TECHNICAL_AUTO_RETRY_LIMIT: u32 = 2;

pub(super) fn visual_note_is_retryable(note: Option<&str>) -> bool {
    match note.unwrap_or("") {
        "visual_analysis_skipped_by_user"
        | "visual_analysis_not_applicable"
        | "visual_task_input_invalid" => false,
        note if note.starts_with("provider_http_4") && note != "provider_http_429" => false,
        _ => true,
    }
}

pub(super) fn technical_error_is_retryable(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    error.contains("10060")
        || error.contains("超时")
        || error.contains("没有正确答复")
        || error.contains("连接尝试失败")
        || lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("failed to start")
}

pub(super) fn increment_visual_retry(metadata: &mut TechnicalMetadata) -> bool {
    metadata.visual_analysis_retry_count = metadata.visual_analysis_retry_count.saturating_add(1);
    metadata.visual_analysis_retry_count <= VISUAL_AUTO_RETRY_LIMIT
}

pub(super) fn increment_technical_retry(metadata: &mut TechnicalMetadata) -> bool {
    metadata.analysis_retry_count = metadata.analysis_retry_count.saturating_add(1);
    metadata.analysis_retry_count <= TECHNICAL_AUTO_RETRY_LIMIT
}

pub(super) fn reset_visual_retry(metadata: &mut TechnicalMetadata) {
    metadata.visual_analysis_retry_count = 0;
}

pub(super) fn reset_technical_retry(metadata: &mut TechnicalMetadata) {
    metadata.analysis_retry_count = 0;
}

pub(super) fn reset_retry_counts(
    app: &AppHandle,
    asset_ids: &[String],
    visual: bool,
    technical: bool,
) -> Result<(), String> {
    if asset_ids.is_empty() || (!visual && !technical) {
        return Ok(());
    }
    let connection = open_connection(app)?;
    for asset_id in asset_ids {
        let metadata_json: Option<String> = connection
            .query_row(
                "SELECT metadata_json FROM assets WHERE id = ?1",
                params![asset_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some(metadata_json) = metadata_json else {
            continue;
        };
        let mut metadata: TechnicalMetadata =
            serde_json::from_str(&metadata_json).unwrap_or_default();
        if visual {
            reset_visual_retry(&mut metadata);
        }
        if technical {
            reset_technical_retry(&mut metadata);
        }
        connection
            .execute(
                "UPDATE assets SET metadata_json = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    serde_json::to_string(&metadata).map_err(|error| error.to_string())?,
                    now_millis(),
                    asset_id
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// 技术分析瞬时失败则重新入队；次数用尽或文件不在则返回 false，由调用方标失败。
pub(super) fn maybe_requeue_technical_failure(
    app: &AppHandle,
    asset_id: &str,
    source_reference: &str,
    task_id: &str,
    error: &str,
) -> Result<bool, String> {
    if !technical_error_is_retryable(error) || !std::path::Path::new(source_reference).is_file() {
        return Ok(false);
    }
    let connection = open_connection(app)?;
    let (project_id, metadata_json): (String, String) = connection
        .query_row(
            "SELECT project_id, metadata_json FROM assets WHERE id = ?1",
            params![asset_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|error| error.to_string())?;
    let mut metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
    if metadata.analysis_cancelled || metadata.library_removed {
        return Ok(false);
    }
    if !increment_technical_retry(&mut metadata) {
        return Ok(false);
    }
    let timestamp = now_millis();
    connection
        .execute(
            "UPDATE assets SET analysis_status = 'queued', metadata_json = ?1, updated_at = ?2 WHERE id = ?3",
            params![
                serde_json::to_string(&metadata).map_err(|error| error.to_string())?,
                timestamp,
                asset_id
            ],
        )
        .map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE agent_tasks SET status = 'failed', error_message = ?1, updated_at = ?2 WHERE id = ?3 AND tool_name = 'analyze_asset'",
            params![error, timestamp, task_id],
        )
        .map_err(|error| error.to_string())?;
    let retry_task_id = uuid::Uuid::new_v4().to_string();
    connection
        .execute(
            "INSERT INTO agent_tasks (id, project_id, tool_name, status, input_json, created_at, updated_at) VALUES (?1, ?2, 'analyze_asset', 'queued', ?3, ?4, ?4)",
            params![
                retry_task_id,
                project_id,
                serde_json::json!({ "assetId": asset_id }).to_string(),
                timestamp
            ],
        )
        .map_err(|error| error.to_string())?;
    drop(connection);
    super::analysis::spawn_technical_analysis_tasks(
        app.clone(),
        vec![(asset_id.to_owned(), retry_task_id)],
    );
    log::info!("Technical analysis auto-retry queued for asset {asset_id}.");
    Ok(true)
}

/// 瞬时失败则加次数并重新入队；次数用尽才保持失败。不足 6 条也立即送审。
pub(super) fn conclude_visual_batch(
    app: &AppHandle,
    task_id: &str,
    asset_ids: &[String],
    requested_count: usize,
    note: &str,
) -> Result<(), String> {
    if !visual_note_is_retryable(Some(note)) {
        mark_visual_assets_failed(app, asset_ids, note)?;
        super::visual::close_visual_batch_task(
            app,
            task_id,
            "failed",
            requested_count,
            asset_ids.len(),
            Some(note),
        )?;
        return Ok(());
    }
    let connection = open_connection(app)?;
    let mut retry_ids = Vec::new();
    let mut exhausted_ids = Vec::new();
    for asset_id in asset_ids {
        let metadata_json: Option<String> = connection
            .query_row(
                "SELECT metadata_json FROM assets WHERE id = ?1",
                params![asset_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some(metadata_json) = metadata_json else {
            continue;
        };
        let mut metadata: TechnicalMetadata =
            serde_json::from_str(&metadata_json).unwrap_or_default();
        if metadata.analysis_cancelled || metadata.library_removed {
            continue;
        }
        if metadata.visual_analysis_status == "skipped"
            && metadata.visual_analysis_note.as_deref() == Some("visual_analysis_skipped_by_user")
        {
            continue;
        }
        if increment_visual_retry(&mut metadata) {
            metadata.visual_analysis_status = "queued".to_owned();
            metadata.visual_analysis_note = Some(note.to_owned());
            retry_ids.push(asset_id.clone());
        } else {
            metadata.visual_analysis_status = "failed".to_owned();
            metadata.visual_analysis_note = Some(note.to_owned());
            exhausted_ids.push(asset_id.clone());
        }
        connection
            .execute(
                "UPDATE assets SET metadata_json = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    serde_json::to_string(&metadata).map_err(|error| error.to_string())?,
                    now_millis(),
                    asset_id
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    drop(connection);
    let task_status = if retry_ids.is_empty() {
        "failed"
    } else {
        "completed"
    };
    super::visual::close_visual_batch_task(
        app,
        task_id,
        task_status,
        requested_count,
        exhausted_ids.len(),
        Some(note),
    )?;
    if !retry_ids.is_empty() {
        log::info!(
            "Visual analysis auto-retry {} asset(s) after {note}; sending remainder of {}.",
            retry_ids.len(),
            retry_ids.len()
        );
        queue_visual_analysis_batch(app, &retry_ids)?;
    }
    Ok(())
}

/// 启动或队列空闲时，把仍可补跑的失败/跳过项重新入队；1–5 条也发。
pub(super) fn requeue_retryable_visual_failures(app: &AppHandle) -> Result<usize, String> {
    let connection = open_connection(app)?;
    let rows = connection
        .prepare(
            "SELECT id, project_id, kind, metadata_json FROM assets
             WHERE analysis_status = 'ready' AND kind IN ('video', 'image')
             AND coalesce(json_extract(metadata_json, '$.libraryRemoved'), 0) = 0
             AND coalesce(json_extract(metadata_json, '$.analysisCancelled'), 0) = 0
             AND json_extract(metadata_json, '$.visualAnalysisStatus') IN ('failed', 'skipped')",
        )
        .map_err(|error| error.to_string())?
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(connection);
    let mut by_project = HashMap::<String, Vec<String>>::new();
    for (asset_id, project_id, kind, metadata_json) in rows {
        let metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
        if metadata.visual_analysis_retry_count >= VISUAL_AUTO_RETRY_LIMIT {
            continue;
        }
        if !visual_note_is_retryable(metadata.visual_analysis_note.as_deref()) {
            continue;
        }
        if super::visual::collect_visual_units(&asset_id, &kind, &metadata).is_empty() {
            continue;
        }
        if has_active_visual_task(app, &asset_id) {
            continue;
        }
        by_project.entry(project_id).or_default().push(asset_id);
    }
    let mut queued = 0usize;
    for asset_ids in by_project.into_values() {
        queued += asset_ids.len();
        queue_visual_analysis_batch(app, &asset_ids)?;
    }
    if queued > 0 {
        log::info!("Visual analysis auto-retry queued {queued} leftover asset(s), including batches smaller than {VISUAL_ANALYSIS_BATCH_SIZE}.");
    }
    Ok(queued)
}

fn mark_visual_assets_failed(
    app: &AppHandle,
    asset_ids: &[String],
    note: &str,
) -> Result<(), String> {
    if asset_ids.is_empty() {
        return Ok(());
    }
    let connection = open_connection(app)?;
    for asset_id in asset_ids {
        let metadata_json: Option<String> = connection
            .query_row(
                "SELECT metadata_json FROM assets WHERE id = ?1",
                params![asset_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some(metadata_json) = metadata_json else {
            continue;
        };
        let mut metadata: TechnicalMetadata =
            serde_json::from_str(&metadata_json).unwrap_or_default();
        if metadata.analysis_cancelled || metadata.library_removed {
            continue;
        }
        if metadata.visual_analysis_status == "skipped"
            && metadata.visual_analysis_note.as_deref() == Some("visual_analysis_skipped_by_user")
        {
            continue;
        }
        metadata.visual_analysis_status = "failed".to_owned();
        metadata.visual_analysis_note = Some(note.to_owned());
        connection
            .execute(
                "UPDATE assets SET metadata_json = ?1, updated_at = ?2 WHERE id = ?3",
                params![
                    serde_json::to_string(&metadata).map_err(|error| error.to_string())?,
                    now_millis(),
                    asset_id
                ],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn has_active_visual_task(app: &AppHandle, asset_id: &str) -> bool {
    open_connection(app)
        .ok()
        .and_then(|connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM agent_tasks WHERE tool_name = 'analyze_asset_visual_batch' AND status IN ('queued', 'running') AND instr(input_json, ?1) > 0",
                    params![asset_id],
                    |row| row.get::<_, i64>(0),
                )
                .ok()
        })
        .unwrap_or(0)
        > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remainder_under_six_is_a_valid_send_size() {
        assert!(VISUAL_ANALYSIS_BATCH_SIZE > 4);
        assert!((1..VISUAL_ANALYSIS_BATCH_SIZE).all(|count| count > 0));
    }

    #[test]
    fn retryable_notes_exclude_user_skip_and_client_errors() {
        assert!(!visual_note_is_retryable(Some(
            "visual_analysis_skipped_by_user"
        )));
        assert!(!visual_note_is_retryable(Some(
            "visual_analysis_not_applicable"
        )));
        assert!(!visual_note_is_retryable(Some("provider_http_400")));
        assert!(visual_note_is_retryable(Some("provider_timeout")));
        assert!(visual_note_is_retryable(Some("provider_network")));
        assert!(visual_note_is_retryable(Some("provider_unknown")));
        assert!(visual_note_is_retryable(Some("provider_http_429")));
        assert!(visual_note_is_retryable(None));
    }

    #[test]
    fn visual_retry_stops_after_limit() {
        let mut metadata = TechnicalMetadata::default();
        assert!(increment_visual_retry(&mut metadata));
        assert!(increment_visual_retry(&mut metadata));
        assert!(increment_visual_retry(&mut metadata));
        assert!(!increment_visual_retry(&mut metadata));
        assert_eq!(metadata.visual_analysis_retry_count, 4);
        reset_visual_retry(&mut metadata);
        assert_eq!(metadata.visual_analysis_retry_count, 0);
    }

    #[test]
    fn technical_timeout_is_retryable() {
        assert!(technical_error_is_retryable(
            "由于连接方在一段时间后没有正确答复或连接的主机没有反应，连接尝试失败。 (os error 10060)"
        ));
        assert!(technical_error_is_retryable("FFprobe timed out while reading this media file."));
        assert!(!technical_error_is_retryable(
            "The media asset is no longer available."
        ));
    }
}
