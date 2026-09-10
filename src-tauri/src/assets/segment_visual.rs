//! 片段级视觉分析：按需入队、帧装箱、ensure 等待与永久缓存。
//! 与素材级 analyze_asset_visual_batch 并存；共用 visual worker 与熔断。

use crate::db::{now_millis, open_connection};
use crate::models::{TechnicalMetadata, VisualEvidence};
use crate::provider::{
    complete_visual_model_request, model_response_json_text, post_visual_model_payload,
    visual_model_retry_after, ModelAccess,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, OptionalExtension};
use serde::{Deserialize, Deserializer};
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    fs, thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

use super::segments::CURRENT_ANALYSIS_VERSION;
use super::visual::{spawn_visual_analysis_worker, VISUAL_ANALYSIS_BATCH_SIZE};

pub(crate) const CURRENT_SEGMENT_VISUAL_VERSION: u32 = 2;
pub(crate) const SEGMENT_VISUAL_FRAME_BATCH: usize = 12;
const SEGMENT_VISUAL_TIMEOUT: Duration = Duration::from_secs(45);
pub(crate) const DEFAULT_ENSURE_BUDGET: Duration = Duration::from_secs(150);

#[derive(Debug, Clone, Default)]
pub(crate) struct EnsureSegmentVisualResult {
    pub ready: Vec<String>,
    pub pending: Vec<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SegmentVisualResponse {
    #[serde(default)]
    assets: Vec<SegmentVisualAsset>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SegmentVisualAsset {
    asset_id: String,
    #[serde(default)]
    segments: Vec<SegmentVisualItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SegmentVisualItem {
    segment_id: String,
    /// 模型偶发返回 [startMs, endMs]；取首个数值（或区间中点）作为代表时刻。
    #[serde(default, deserialize_with = "i64_or_range")]
    time_ms: Option<i64>,
    #[serde(default, deserialize_with = "string_or_string_vec")]
    subjects: Vec<String>,
    /// 模型偶发把 scene 写成字符串数组。
    #[serde(default, deserialize_with = "string_or_joined")]
    scene: Option<String>,
    #[serde(default, deserialize_with = "string_or_string_vec")]
    actions: Vec<String>,
    #[serde(default, deserialize_with = "string_or_string_vec")]
    products: Vec<String>,
    /// Agnes 等模型常把 qualityNotes 写成单个字符串，需兼容数组与字符串。
    #[serde(default, deserialize_with = "string_or_string_vec")]
    quality_notes: Vec<String>,
    #[serde(default, deserialize_with = "string_or_joined")]
    shot_type: Option<String>,
    #[serde(default, deserialize_with = "string_or_joined")]
    camera_motion: Option<String>,
}

pub(crate) fn string_or_joined<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let parts = string_or_string_vec(deserializer)?;
    if parts.is_empty() {
        Ok(None)
    } else {
        Ok(Some(parts.join(", ")))
    }
}

pub(crate) fn i64_or_range<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Null => None,
        serde_json::Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|float| float.round() as i64)),
        serde_json::Value::Array(items) => {
            let nums: Vec<i64> = items
                .into_iter()
                .filter_map(|item| match item {
                    serde_json::Value::Number(number) => number
                        .as_i64()
                        .or_else(|| number.as_f64().map(|float| float.round() as i64)),
                    serde_json::Value::String(text) => text.trim().parse().ok(),
                    _ => None,
                })
                .collect();
            match nums.as_slice() {
                [] => None,
                [only] => Some(*only),
                [start, end, ..] => Some((*start + *end) / 2),
            }
        }
        serde_json::Value::String(text) => text.trim().parse().ok(),
        _ => None,
    })
}

pub(crate) fn string_or_string_vec<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(Vec::new()),
        serde_json::Value::String(text) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                Ok(Vec::new())
            } else {
                Ok(vec![trimmed.to_owned()])
            }
        }
        serde_json::Value::Array(items) => Ok(items
            .into_iter()
            .filter_map(|item| match item {
                serde_json::Value::String(text) => {
                    let trimmed = text.trim();
                    (!trimmed.is_empty()).then(|| trimmed.to_owned())
                }
                other => {
                    let text = other.to_string();
                    let trimmed = text.trim().trim_matches('"');
                    (!trimmed.is_empty()).then(|| trimmed.to_owned())
                }
            })
            .collect()),
        other => {
            let text = other.to_string();
            let trimmed = text.trim().trim_matches('"');
            if trimmed.is_empty() {
                Ok(Vec::new())
            } else {
                Ok(vec![trimmed.to_owned()])
            }
        }
    }
}

pub(crate) fn asset_needs_segment_visual(metadata: &TechnicalMetadata) -> bool {
    metadata.analysis_version >= CURRENT_ANALYSIS_VERSION
        && metadata.visual_analysis_version < CURRENT_SEGMENT_VISUAL_VERSION
        && metadata
            .scene_segments
            .iter()
            .any(|segment| !segment.id.is_empty() && !segment.frames.is_empty())
}

pub(crate) fn queue_segment_visual_batch(
    app: &AppHandle,
    asset_ids: &[String],
) -> Result<(), String> {
    if asset_ids.is_empty() {
        return Ok(());
    }
    let connection = open_connection(app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let mut queued = Vec::new();
    let mut project_id = None;
    for asset_id in asset_ids {
        let row = transaction
            .query_row(
                "SELECT project_id, kind, metadata_json FROM assets WHERE id = ?1 AND analysis_status = 'ready'",
                params![asset_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((row_project_id, kind, metadata_json)) = row else {
            continue;
        };
        if kind != "video" {
            continue;
        }
        let metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
        if !asset_needs_segment_visual(&metadata) {
            continue;
        }
        let already: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM agent_tasks WHERE tool_name = 'analyze_asset_segments_batch' AND status IN ('queued', 'running') AND instr(input_json, ?1) > 0",
                params![asset_id],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if already > 0 {
            continue;
        }
        project_id.get_or_insert(row_project_id);
        queued.push(asset_id.clone());
    }
    if let Some(project_id) = project_id.as_ref().filter(|_| !queued.is_empty()) {
        for batch in queued.chunks(VISUAL_ANALYSIS_BATCH_SIZE) {
            transaction
                .execute(
                    "INSERT INTO agent_tasks (id, project_id, tool_name, status, input_json, result_json, created_at, updated_at) VALUES (?1, ?2, 'analyze_asset_segments_batch', 'queued', ?3, ?4, ?5, ?5)",
                    params![
                        Uuid::new_v4().to_string(),
                        project_id,
                        json!({ "assetIds": batch }).to_string(),
                        json!({ "requestedCount": batch.len(), "readyCount": 0, "skippedCount": 0, "failedCount": 0, "priority": 100 }).to_string(),
                        now_millis(),
                    ],
                )
                .map_err(|error| error.to_string())?;
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    if project_id.is_some() {
        spawn_visual_analysis_worker(app.clone());
    }
    Ok(())
}

/// 按需确保片段视觉证据。超时素材进入 pending，任务继续后台跑。
pub(crate) fn ensure_segment_visual_evidence(
    app: &AppHandle,
    project_id: &str,
    asset_ids: &[String],
    budget: Duration,
) -> Result<EnsureSegmentVisualResult, String> {
    let mut ready = Vec::new();
    let mut need = Vec::new();
    let connection = open_connection(app)?;
    for asset_id in asset_ids {
        let metadata_json: Option<String> = connection
            .query_row(
                "SELECT metadata_json FROM assets WHERE id = ?1 AND project_id = ?2 AND analysis_status = 'ready'",
                params![asset_id, project_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some(metadata_json) = metadata_json else {
            continue;
        };
        let metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
        if metadata.analysis_version < CURRENT_ANALYSIS_VERSION {
            // 尚未分段：按整条参与。
            ready.push(asset_id.clone());
        } else if metadata.visual_analysis_version >= CURRENT_SEGMENT_VISUAL_VERSION {
            ready.push(asset_id.clone());
        } else if asset_needs_segment_visual(&metadata) {
            need.push(asset_id.clone());
        } else {
            ready.push(asset_id.clone());
        }
    }
    drop(connection);
    if need.is_empty() {
        return Ok(EnsureSegmentVisualResult {
            ready,
            pending: Vec::new(),
        });
    }
    queue_segment_visual_batch(app, &need)?;
    let deadline = Instant::now() + crate::execution_deadline::timeout(budget)?;
    let mut remaining: HashSet<String> = need.iter().cloned().collect();
    while !remaining.is_empty() && Instant::now() < deadline {
        if visual_model_retry_after().is_some() {
            break;
        }
        spawn_visual_analysis_worker(app.clone());
        let connection = open_connection(app)?;
        let mut newly_ready = Vec::new();
        for asset_id in remaining.iter() {
            let metadata_json: String = connection
                .query_row(
                    "SELECT metadata_json FROM assets WHERE id = ?1",
                    params![asset_id],
                    |row| row.get(0),
                )
                .unwrap_or_default();
            let metadata: TechnicalMetadata =
                serde_json::from_str(&metadata_json).unwrap_or_default();
            if metadata.visual_analysis_version >= CURRENT_SEGMENT_VISUAL_VERSION {
                newly_ready.push(asset_id.clone());
            }
        }
        drop(connection);
        for asset_id in newly_ready {
            remaining.remove(&asset_id);
            ready.push(asset_id);
        }
        if remaining.is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(200));
    }
    let pending = remaining.into_iter().collect::<Vec<_>>();
    log::info!(
        "ensure_segment_visual_evidence: project={}, requested={}, ready={}, pending={}",
        project_id,
        asset_ids.len(),
        ready.len(),
        pending.len()
    );
    Ok(EnsureSegmentVisualResult { ready, pending })
}

fn segment_visual_model_content(
    frames: &[(String, String, i64, Vec<u8>)],
) -> Vec<serde_json::Value> {
    let mut content = vec![json!({
        "type": "input_text",
        "text": "Analyze only visible evidence for each frame. Return JSON {assets:[{assetId,segments:[{segmentId,timeMs,subjects,scene,actions,products,shotType,cameraMotion,qualityNotes}]}]}. timeMs must be a single integer matching sourceTimeMs (not a range). scene/shotType/cameraMotion must be single strings. qualityNotes/subjects/actions/products must be string arrays. shotType must be one of wide|medium|close-up|detail. cameraMotion must be one of static|pan|tilt|handheld|zoom. Each assetId/segmentId/timeMs must match a supplied label. Do not infer facts not visible."
    })];
    for (asset_id, segment_id, time_ms, image) in frames {
        content.push(json!({
            "type": "input_text",
            "text": format!("assetId={asset_id}; segmentId={segment_id}; sourceTimeMs={time_ms}")
        }));
        content.push(json!({
            "type": "input_image",
            "image_url": format!("data:image/jpeg;base64,{}", STANDARD.encode(image))
        }));
    }
    content
}

fn update_segment_batch_task(
    app: &AppHandle,
    task_id: &str,
    status: &str,
    requested: usize,
    ready: usize,
    failed: usize,
    error: Option<&str>,
) -> Result<(), String> {
    open_connection(app)?
        .execute(
            "UPDATE agent_tasks SET status = ?1, result_json = json_set(coalesce(result_json, '{}'), '$.requestedCount', ?2, '$.readyCount', ?3, '$.failedCount', ?4), error_message = ?5, updated_at = ?6 WHERE id = ?7",
            params![
                status,
                requested as i64,
                ready as i64,
                failed as i64,
                error,
                now_millis(),
                task_id
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn run_segment_visual_analysis_batch(
    app: AppHandle,
    task_id: String,
    asset_ids: Vec<String>,
) {
    let requested = asset_ids.len();
    let _ = update_segment_batch_task(&app, &task_id, "running", requested, 0, 0, None);
    let assets = (|| -> Result<Vec<(String, TechnicalMetadata, String)>, String> {
        let connection = open_connection(&app)?;
        let mut out = Vec::new();
        for asset_id in &asset_ids {
            let (metadata_json,): (String,) = connection
                .query_row(
                    "SELECT metadata_json FROM assets WHERE id = ?1 AND analysis_status = 'ready'",
                    params![asset_id],
                    |row| Ok((row.get(0)?,)),
                )
                .map_err(|_| "visual_asset_unavailable".to_owned())?;
            let metadata: TechnicalMetadata =
                serde_json::from_str(&metadata_json).unwrap_or_default();
            out.push((asset_id.clone(), metadata, metadata_json));
        }
        Ok(out)
    })();
    let Ok(assets) = assets else {
        let _ = update_segment_batch_task(
            &app,
            &task_id,
            "failed",
            requested,
            0,
            requested,
            Some("visual_asset_unavailable"),
        );
        return;
    };

    let mut frames: Vec<(String, String, i64, Vec<u8>)> = Vec::new();
    for (asset_id, metadata, _) in &assets {
        for segment in &metadata.scene_segments {
            if segment.id.is_empty() || segment.visual_evidence.is_some() {
                continue;
            }
            for frame in &segment.frames {
                let Ok(bytes) = fs::read(&frame.image_path) else {
                    continue;
                };
                frames.push((asset_id.clone(), segment.id.clone(), frame.time_ms, bytes));
            }
        }
    }
    if frames.is_empty() {
        // 无需请求：若片段本就齐全则抬版本。
        for (asset_id, mut metadata, original_json) in assets {
            let all_ready = metadata
                .scene_segments
                .iter()
                .filter(|segment| !segment.id.is_empty())
                .all(|segment| segment.visual_evidence.is_some());
            if all_ready {
                metadata.visual_analysis_version = CURRENT_SEGMENT_VISUAL_VERSION;
                let _ = cas_write_metadata(&app, &asset_id, &original_json, &metadata);
            }
        }
        let _ =
            update_segment_batch_task(&app, &task_id, "completed", requested, requested, 0, None);
        return;
    }

    let access = match ModelAccess::resolve() {
        Ok(access) => access,
        Err(error) => {
            log::warn!("Segment visual analysis: provider unavailable: {error}");
            let _ = update_segment_batch_task(
                &app,
                &task_id,
                "completed",
                requested,
                0,
                requested,
                Some("visual_provider_unavailable"),
            );
            return;
        }
    };

    let mut evidence_by_asset: HashMap<String, HashMap<String, VisualEvidence>> = HashMap::new();
    for chunk in frames.chunks(SEGMENT_VISUAL_FRAME_BATCH) {
        let content = segment_visual_model_content(chunk);
        let request = json!({
            "model": "gpt-5.4",
            "store": false,
            "stream": true,
            "input": [{ "role": "user", "content": content }],
            "text": { "format": { "type": "json_object" } }
        });
        let response_body =
            match post_visual_model_payload(&access, &request, Some(SEGMENT_VISUAL_TIMEOUT)) {
                Ok(body) => body,
                Err(error) if error == "visual_provider_circuit_open" => {
                    let _ = update_segment_batch_task(
                        &app,
                        &task_id,
                        "queued",
                        requested,
                        0,
                        0,
                        Some("visual_provider_cooldown"),
                    );
                    return;
                }
                Err(error) => {
                    log::warn!("Segment visual model request failed: {error}");
                    continue;
                }
            };
        complete_visual_model_request(true);
        let Some(text) = model_response_json_text(&access, &response_body) else {
            log::warn!("Segment visual response did not contain JSON text.");
            continue;
        };
        let parsed = match serde_json::from_str::<SegmentVisualResponse>(&text) {
            Ok(parsed) => parsed,
            Err(error) => {
                log::warn!(
                    "Segment visual response parse failed: text_len={}, serde={}",
                    text.len(),
                    error
                );
                continue;
            }
        };
        for asset in parsed.assets {
            let entry = evidence_by_asset.entry(asset.asset_id).or_default();
            for item in asset.segments {
                entry.insert(
                    item.segment_id.clone(),
                    VisualEvidence {
                        time_ms: item.time_ms,
                        subjects: item.subjects,
                        scene: item.scene,
                        actions: item.actions,
                        products: item.products,
                        quality_notes: item.quality_notes,
                        shot_type: item.shot_type,
                        camera_motion: item.camera_motion,
                        segment_id: Some(item.segment_id),
                    },
                );
            }
        }
    }

    let mut ready_count = 0;
    let mut failed_count = 0;
    let mut project_id = None::<String>;
    for (asset_id, mut metadata, original_json) in assets {
        let Some(segment_map) = evidence_by_asset.get(&asset_id) else {
            failed_count += 1;
            continue;
        };
        for segment in &mut metadata.scene_segments {
            if let Some(evidence) = segment_map.get(&segment.id) {
                segment.visual_evidence = Some(evidence.clone());
            }
        }
        metadata.visual_evidence = metadata
            .scene_segments
            .iter()
            .filter_map(|segment| segment.visual_evidence.clone())
            .collect();
        let all_ready = metadata
            .scene_segments
            .iter()
            .filter(|segment| !segment.id.is_empty() && !segment.frames.is_empty())
            .all(|segment| segment.visual_evidence.is_some());
        if all_ready {
            metadata.visual_analysis_version = CURRENT_SEGMENT_VISUAL_VERSION;
            metadata.visual_analysis_status = "ready".to_owned();
            metadata.visual_analysis_note = None;
            ready_count += 1;
        } else {
            failed_count += 1;
        }
        if cas_write_metadata(&app, &asset_id, &original_json, &metadata).is_ok() {
            if project_id.is_none() {
                if let Ok(connection) = open_connection(&app) {
                    project_id = connection
                        .query_row(
                            "SELECT project_id FROM assets WHERE id = ?1",
                            params![asset_id],
                            |row| row.get(0),
                        )
                        .ok();
                }
            }
            let _ =
                crate::storyboard::semantic::refresh_segment_embeddings(&app, &asset_id, &metadata);
        }
    }
    if let Some(project_id) = project_id {
        let _ = app.emit("assets-changed", project_id);
    }
    let _ = update_segment_batch_task(
        &app,
        &task_id,
        "completed",
        requested,
        ready_count,
        failed_count,
        None,
    );
}

fn cas_write_metadata(
    app: &AppHandle,
    asset_id: &str,
    original_json: &str,
    metadata: &TechnicalMetadata,
) -> Result<(), String> {
    let next = serde_json::to_string(metadata).map_err(|error| error.to_string())?;
    let updated = open_connection(app)?
        .execute(
            "UPDATE assets SET metadata_json = ?1, updated_at = ?2 WHERE id = ?3 AND metadata_json = ?4",
            params![next, now_millis(), asset_id, original_json],
        )
        .map_err(|error| error.to_string())?;
    if updated == 1 {
        Ok(())
    } else {
        Err("segment_visual_cas_conflict".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct FlexibleValues {
        #[serde(deserialize_with = "i64_or_range")]
        time: Option<i64>,
        #[serde(deserialize_with = "string_or_string_vec")]
        tags: Vec<String>,
        #[serde(deserialize_with = "string_or_joined")]
        scene: Option<String>,
    }

    #[test]
    fn visual_response_values_accept_common_provider_variants() {
        let parsed: FlexibleValues = serde_json::from_value(json!({
            "time": [1000, 3000],
            "tags": "battery rack",
            "scene": ["factory", "indoors"]
        }))
        .unwrap();

        assert_eq!(parsed.time, Some(2000));
        assert_eq!(parsed.tags, vec!["battery rack"]);
        assert_eq!(parsed.scene.as_deref(), Some("factory, indoors"));
    }
}
