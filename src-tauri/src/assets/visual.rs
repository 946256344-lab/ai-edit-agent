//! 视觉分析批次队列：远端视觉模型请求、优先级排序、批次worker与恢复。
//! 技术分析完成后自动排队；粗识别按硬切段各送 1 帧、最多 6 段一批，最后不足 6 段也送审。
//! 瞬时失败自动补跑；storyboard brief 可对 pending 批次重新排序。

use crate::db::{now_millis, open_connection};
use crate::models::{BatchAssetActionResult, TechnicalMetadata, VisualEvidence};
use crate::provider::{
    complete_visual_model_request, model_response_json_text, post_visual_model_payload,
    visual_model_retry_after, ModelAccess,
};
use base64::{engine::general_purpose::STANDARD, Engine};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

const VISUAL_ANALYSIS_TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const VISUAL_ANALYSIS_BATCH_SIZE: usize = 6;

static VISUAL_ANALYSIS_WORKER_ACTIVE: AtomicBool = AtomicBool::new(false);
static VISUAL_ANALYSIS_WAKE_SCHEDULED: AtomicBool = AtomicBool::new(false);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VisualBatchResponse {
    #[serde(default)]
    assets: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VisualBatchAsset {
    asset_id: String,
    /// 模型可能带回 timeMs；绑定只认 assetId+segmentId，本地抽帧时间为准。
    #[allow(dead_code)]
    #[serde(
        default,
        deserialize_with = "crate::assets::segment_visual::i64_or_range"
    )]
    time_ms: Option<i64>,
    #[serde(
        default,
        deserialize_with = "crate::assets::segment_visual::string_or_string_vec"
    )]
    subjects: Vec<String>,
    #[serde(
        default,
        deserialize_with = "crate::assets::segment_visual::string_or_joined"
    )]
    scene: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::assets::segment_visual::string_or_string_vec"
    )]
    actions: Vec<String>,
    #[serde(
        default,
        deserialize_with = "crate::assets::segment_visual::string_or_string_vec"
    )]
    products: Vec<String>,
    #[serde(
        default,
        deserialize_with = "crate::assets::segment_visual::string_or_string_vec"
    )]
    quality_notes: Vec<String>,
    #[serde(
        default,
        deserialize_with = "crate::assets::segment_visual::string_or_joined"
    )]
    segment_id: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::assets::segment_visual::string_or_joined"
    )]
    narrative_role: Option<String>,
    #[serde(
        default,
        deserialize_with = "crate::assets::segment_visual::string_or_joined"
    )]
    caption: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CoarseVisualUnit {
    pub asset_id: String,
    pub segment_id: String,
    pub time_ms: Option<i64>,
    pub image_path: String,
}

/// 粗识别发送单位：有硬切段则每段中点 1 帧；否则退回整条代表帧。
pub(crate) fn collect_visual_units(
    asset_id: &str,
    kind: &str,
    metadata: &TechnicalMetadata,
) -> Vec<CoarseVisualUnit> {
    if kind == "video" {
        let mut units = Vec::new();
        for segment in &metadata.scene_segments {
            if segment.id.is_empty() {
                continue;
            }
            let Some(frame) = segment.frames.get(segment.frames.len() / 2) else {
                continue;
            };
            units.push(CoarseVisualUnit {
                asset_id: asset_id.to_owned(),
                segment_id: segment.id.clone(),
                time_ms: Some(frame.time_ms),
                image_path: frame.image_path.clone(),
            });
        }
        if !units.is_empty() {
            return units;
        }
    }
    representative_frame(metadata, kind)
        .map(|(image_path, time_ms)| CoarseVisualUnit {
            asset_id: asset_id.to_owned(),
            segment_id: String::new(),
            time_ms,
            image_path,
        })
        .into_iter()
        .collect()
}

fn unit_key(asset_id: &str, segment_id: &str) -> String {
    format!("{asset_id}\0{segment_id}")
}

fn unit_key_segment(key: &str) -> &str {
    key.split_once('\0')
        .map(|(_, segment_id)| segment_id)
        .unwrap_or("")
}

fn nonempty_text(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| text.chars().take(800).collect())
}

fn source_position_hint(time_ms: Option<i64>, duration_ms: Option<i64>) -> Option<String> {
    let time = time_ms?;
    let duration = duration_ms.filter(|value| *value > 0)?;
    let percent = ((time as f64 / duration as f64) * 100.0)
        .clamp(0.0, 100.0)
        .round() as i64;
    Some(format!("about {percent}% of the source"))
}

fn bind_coarse_visual_key(
    item: &VisualBatchAsset,
    expected: &HashMap<String, Option<i64>>,
) -> Option<(String, Option<i64>)> {
    let segment_id = item.segment_id.as_deref().unwrap_or("");
    let key = unit_key(&item.asset_id, segment_id);
    if let Some(time_ms) = expected.get(&key) {
        return Some((key, *time_ms));
    }
    if !segment_id.is_empty() {
        return None;
    }
    let prefix = format!("{}\0", item.asset_id);
    let mut matches = expected
        .iter()
        .filter(|(candidate, _)| candidate.starts_with(&prefix));
    let (key, time_ms) = matches.next()?;
    if matches.next().is_some() {
        return None;
    }
    Some((key.clone(), *time_ms))
}

fn coarse_visual_card(
    item: &VisualBatchAsset,
    segment_id: &str,
    time_ms: Option<i64>,
) -> VisualEvidence {
    VisualEvidence {
        time_ms,
        subjects: item
            .subjects
            .iter()
            .filter_map(|value| nonempty_text(Some(value)))
            .collect(),
        scene: nonempty_text(item.scene.as_deref()),
        actions: item
            .actions
            .iter()
            .filter_map(|value| nonempty_text(Some(value)))
            .collect(),
        products: item
            .products
            .iter()
            .filter_map(|value| nonempty_text(Some(value)))
            .collect(),
        quality_notes: item
            .quality_notes
            .iter()
            .filter_map(|value| nonempty_text(Some(value)))
            .collect(),
        shot_type: None,
        camera_motion: None,
        segment_id: nonempty_text(Some(segment_id)),
        narrative_role: nonempty_text(item.narrative_role.as_deref()),
        caption: nonempty_text(item.caption.as_deref()),
    }
}

fn coarse_visual_task_payload(batch: &[CoarseVisualUnit]) -> Value {
    let mut asset_ids = Vec::new();
    let mut seen = HashSet::new();
    let mut segments = Vec::new();
    for unit in batch {
        if seen.insert(unit.asset_id.clone()) {
            asset_ids.push(unit.asset_id.clone());
        }
        segments.push(serde_json::json!({
            "assetId": unit.asset_id,
            "segmentId": unit.segment_id,
        }));
    }
    serde_json::json!({
        "assetIds": asset_ids,
        "segments": segments,
    })
}

fn parse_asset_ids(value: &Value) -> Option<Vec<String>> {
    value.get("assetIds").and_then(Value::as_array).map(|ids| {
        ids.iter()
            .filter_map(|id| id.as_str().map(str::to_owned))
            .collect::<Vec<_>>()
    })
}

fn parse_coarse_visual_specs(value: &Value) -> Option<Vec<(String, String)>> {
    if let Some(segments) = value.get("segments").and_then(Value::as_array) {
        if segments.is_empty() || segments.len() > VISUAL_ANALYSIS_BATCH_SIZE {
            return None;
        }
        let mut specs = Vec::new();
        for segment in segments {
            let asset_id = segment.get("assetId").and_then(Value::as_str)?.to_owned();
            let segment_id = segment
                .get("segmentId")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned();
            specs.push((asset_id, segment_id));
        }
        return Some(specs);
    }
    let asset_ids = parse_asset_ids(value)?;
    if asset_ids.is_empty() || asset_ids.len() > VISUAL_ANALYSIS_BATCH_SIZE {
        return None;
    }
    Some(
        asset_ids
            .into_iter()
            .map(|asset_id| (asset_id, String::new()))
            .collect(),
    )
}

fn coarse_visual_is_complete(kind: &str, metadata: &TechnicalMetadata) -> bool {
    let units = collect_visual_units("asset", kind, metadata);
    if units.is_empty() {
        return false;
    }
    units.iter().all(|unit| {
        if unit.segment_id.is_empty() {
            !metadata.visual_evidence.is_empty()
        } else {
            metadata
                .scene_segments
                .iter()
                .any(|segment| segment.id == unit.segment_id && segment.visual_evidence.is_some())
        }
    })
}

fn merge_coarse_visual_cards(metadata: &mut TechnicalMetadata, cards: &[VisualEvidence]) {
    for card in cards {
        let Some(segment_id) = card
            .segment_id
            .as_deref()
            .filter(|segment_id| !segment_id.is_empty())
        else {
            if !metadata
                .scene_segments
                .iter()
                .any(|segment| !segment.id.is_empty() && !segment.frames.is_empty())
            {
                metadata.visual_evidence = vec![card.clone()];
            }
            continue;
        };
        if let Some(segment) = metadata
            .scene_segments
            .iter_mut()
            .find(|segment| segment.id == segment_id)
        {
            segment.visual_evidence = Some(card.clone());
        }
    }
    if metadata
        .scene_segments
        .iter()
        .any(|segment| !segment.id.is_empty() && !segment.frames.is_empty())
    {
        metadata.visual_evidence = metadata
            .scene_segments
            .iter()
            .filter_map(|segment| segment.visual_evidence.clone())
            .collect();
    }
}

#[derive(Clone)]
struct VisualBatchRanking {
    task_id: String,
    created_at: i64,
    priority: usize,
}

// representative_frame 从 analysis 模块导入
use super::analysis::representative_frame;

pub(super) fn close_visual_batch_task(
    app: &AppHandle,
    task_id: &str,
    status: &str,
    requested_count: usize,
    failed_count: usize,
    error_code: Option<&str>,
) -> Result<(), String> {
    update_visual_batch_task(
        app,
        task_id,
        status,
        requested_count,
        0,
        0,
        failed_count,
        error_code,
    )
}

fn fail_or_retry_visual_batch(
    app: &AppHandle,
    task_id: &str,
    asset_ids: &[String],
    requested_count: usize,
    note: &str,
) {
    if let Err(error) =
        super::retry::conclude_visual_batch(app, task_id, asset_ids, requested_count, note)
    {
        log::warn!("Visual batch conclude failed: {error}");
        let _ = update_visual_metadata(
            app,
            Some(task_id),
            asset_ids,
            "failed",
            &HashMap::new(),
            Some(note),
        );
        let _ = update_visual_batch_task(
            app,
            task_id,
            "failed",
            requested_count,
            0,
            0,
            requested_count,
            Some(note),
        );
    }
}

fn update_visual_batch_task(
    app: &AppHandle,
    task_id: &str,
    status: &str,
    requested_count: usize,
    ready_count: usize,
    skipped_count: usize,
    failed_count: usize,
    error_code: Option<&str>,
) -> Result<(), String> {
    let connection = open_connection(app)?;
    let created_at = connection
        .query_row(
            "SELECT created_at FROM agent_tasks WHERE id = ?1",
            params![task_id],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or_else(|e| {
            log::warn!("Visual batch task timestamp unreadable: {e}");
            now_millis()
        });
    let timestamp = now_millis();
    connection
        .execute(
            "UPDATE agent_tasks SET status = ?1, result_json = ?2, error_message = ?3, updated_at = ?4 WHERE id = ?5 AND status != 'cancelled'",
            params![
                status,
                serde_json::json!({
                    "requestedCount": requested_count,
                    "readyCount": ready_count,
                    "skippedCount": skipped_count,
                    "failedCount": failed_count,
                    "durationMs": timestamp.saturating_sub(created_at),
                })
                .to_string(),
                error_code,
                timestamp,
                task_id,
            ],
        )
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn preserves_explicit_visual_skip(metadata: &TechnicalMetadata, next_status: &str) -> bool {
    metadata.visual_analysis_status == "skipped"
        && metadata.visual_analysis_note.as_deref() == Some("visual_analysis_skipped_by_user")
        && next_status != "skipped"
}

fn update_visual_metadata(
    app: &AppHandle,
    task_id: Option<&str>,
    asset_ids: &[String],
    status: &str,
    evidence: &HashMap<String, VisualEvidence>,
    note: Option<&str>,
) -> Result<(), String> {
    let connection = open_connection(app)?;
    let mut updates = Vec::new();
    let mut embedding_unavailable = false;
    for asset_id in asset_ids {
        let metadata_json: String = connection
            .query_row(
                "SELECT metadata_json FROM assets WHERE id = ?1",
                params![asset_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .unwrap_or_else(|| "{}".to_owned());
        let mut metadata: TechnicalMetadata =
            serde_json::from_str(&metadata_json).unwrap_or_default();
        if metadata.analysis_cancelled
            || metadata.library_removed
            || preserves_explicit_visual_skip(&metadata, status)
        {
            continue;
        }
        metadata.visual_analysis_status = status.to_owned();
        metadata.visual_analysis_note = note.map(str::to_owned);
        if let Some(item) = evidence.get(asset_id) {
            metadata.visual_evidence = vec![item.clone()];
        } else if status == "skipped" {
            metadata.visual_evidence.clear();
            for segment in &mut metadata.scene_segments {
                segment.visual_evidence = None;
            }
        }
        if crate::storyboard::semantic::refresh_metadata_embedding(app, &mut metadata).is_err() {
            embedding_unavailable = true;
        }
        updates.push((
            asset_id.clone(),
            metadata_json,
            serde_json::to_string(&metadata).map_err(|error| error.to_string())?,
        ));
    }
    if embedding_unavailable {
        log::warn!(
            "Local semantic embedding unavailable; lexical storyboard ranking remains active."
        );
    }
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    for (asset_id, original_metadata_json, metadata_json) in updates {
        let updated = transaction
            .execute(
                "UPDATE assets SET metadata_json = ?1, updated_at = ?2 WHERE id = ?3 AND metadata_json = ?4 AND (?5 IS NULL OR EXISTS (SELECT 1 FROM agent_tasks WHERE id = ?5 AND status = 'running'))",
                params![metadata_json, now_millis(), asset_id, original_metadata_json, task_id],
            )
            .map_err(|error| error.to_string())?;
        if updated == 0 {
            return Err("Visual metadata changed while analysis was completing.".to_owned());
        }
    }
    transaction.commit().map_err(|error| error.to_string())
}

fn has_other_coarse_visual_task(app: &AppHandle, asset_id: &str, except_task_id: &str) -> bool {
    open_connection(app)
        .ok()
        .and_then(|connection| {
            connection
                .query_row(
                    "SELECT COUNT(*) FROM agent_tasks WHERE tool_name = 'analyze_asset_visual_batch' AND status IN ('queued', 'running') AND id != ?1 AND instr(input_json, ?2) > 0",
                    params![except_task_id, asset_id],
                    |row| row.get::<_, i64>(0),
                )
                .ok()
        })
        .unwrap_or(0)
        > 0
}

fn commit_coarse_visual_cards(
    app: &AppHandle,
    task_id: &str,
    kind_by_asset: &HashMap<String, String>,
    cards_by_asset: &HashMap<String, Vec<VisualEvidence>>,
    failed_asset_ids: &[String],
) -> Result<(), String> {
    let mut asset_ids = cards_by_asset.keys().cloned().collect::<Vec<_>>();
    for asset_id in failed_asset_ids {
        if !asset_ids.iter().any(|id| id == asset_id) {
            asset_ids.push(asset_id.clone());
        }
    }
    let connection = open_connection(app)?;
    let mut updates = Vec::new();
    let mut embedding_unavailable = false;
    for asset_id in &asset_ids {
        let metadata_json: String = connection
            .query_row(
                "SELECT metadata_json FROM assets WHERE id = ?1",
                params![asset_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| error.to_string())?
            .unwrap_or_else(|| "{}".to_owned());
        let mut metadata: TechnicalMetadata =
            serde_json::from_str(&metadata_json).unwrap_or_default();
        if metadata.analysis_cancelled
            || metadata.library_removed
            || preserves_explicit_visual_skip(&metadata, "ready")
        {
            continue;
        }
        if let Some(cards) = cards_by_asset.get(asset_id) {
            merge_coarse_visual_cards(&mut metadata, cards);
        }
        let kind = kind_by_asset
            .get(asset_id)
            .map(String::as_str)
            .unwrap_or("video");
        if coarse_visual_is_complete(kind, &metadata) {
            metadata.visual_analysis_status = "ready".to_owned();
            metadata.visual_analysis_note = None;
        } else if failed_asset_ids.iter().any(|id| id == asset_id)
            && !has_other_coarse_visual_task(app, asset_id, task_id)
        {
            metadata.visual_analysis_status = "failed".to_owned();
            metadata.visual_analysis_note = Some("visual_response_incomplete".to_owned());
        } else {
            metadata.visual_analysis_status = "queued".to_owned();
            metadata.visual_analysis_note = None;
        }
        if crate::storyboard::semantic::refresh_metadata_embedding(app, &mut metadata).is_err() {
            embedding_unavailable = true;
        }
        if crate::storyboard::semantic::refresh_segment_embeddings(app, asset_id, &metadata)
            .is_err()
        {
            embedding_unavailable = true;
        }
        updates.push((
            asset_id.clone(),
            metadata_json,
            serde_json::to_string(&metadata).map_err(|error| error.to_string())?,
        ));
    }
    if embedding_unavailable {
        log::warn!(
            "Local semantic embedding unavailable; lexical storyboard ranking remains active."
        );
    }
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    for (asset_id, original_metadata_json, metadata_json) in updates {
        let updated = transaction
            .execute(
                "UPDATE assets SET metadata_json = ?1, updated_at = ?2 WHERE id = ?3 AND metadata_json = ?4 AND EXISTS (SELECT 1 FROM agent_tasks WHERE id = ?5 AND status = 'running')",
                params![metadata_json, now_millis(), asset_id, original_metadata_json, task_id],
            )
            .map_err(|error| error.to_string())?;
        if updated == 0 {
            return Err("Visual metadata changed while analysis was completing.".to_owned());
        }
    }
    transaction.commit().map_err(|error| error.to_string())
}

fn is_cjk(character: char) -> bool {
    matches!(
        character,
        '\u{3400}'..='\u{4dbf}'
            | '\u{4e00}'..='\u{9fff}'
            | '\u{f900}'..='\u{faff}'
            | '\u{20000}'..='\u{2fa1f}'
    )
}

fn lexical_tokens(text: &str) -> HashSet<String> {
    let mut tokens = HashSet::new();
    let mut ascii = String::new();
    let mut cjk = String::new();
    let flush_ascii = |value: &mut String, tokens: &mut HashSet<String>| {
        if !value.is_empty() {
            tokens.insert(std::mem::take(value));
        }
    };
    let flush_cjk = |value: &mut String, tokens: &mut HashSet<String>| {
        if value.is_empty() {
            return;
        }
        let characters = value.chars().collect::<Vec<_>>();
        tokens.insert(std::mem::take(value));
        if characters.len() > 1 {
            tokens.extend(
                characters
                    .windows(2)
                    .map(|pair| pair.iter().collect::<String>()),
            );
        }
    };
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            flush_cjk(&mut cjk, &mut tokens);
            ascii.push(character.to_ascii_lowercase());
        } else if is_cjk(character) {
            flush_ascii(&mut ascii, &mut tokens);
            cjk.push(character);
        } else {
            flush_ascii(&mut ascii, &mut tokens);
            flush_cjk(&mut cjk, &mut tokens);
        }
    }
    flush_ascii(&mut ascii, &mut tokens);
    flush_cjk(&mut cjk, &mut tokens);
    tokens
}

fn visual_asset_ranking_text(
    display_name: &str,
    source_reference: &str,
    folder_reference: Option<&str>,
    metadata: &TechnicalMetadata,
) -> String {
    let mut hints = vec![display_name.to_owned()];
    if let Some(folder_reference) = folder_reference {
        let folder = Path::new(folder_reference);
        if let Some(folder_name) = folder.file_name().and_then(|name| name.to_str()) {
            hints.push(folder_name.to_owned());
        }
        if let Ok(relative) = Path::new(source_reference).strip_prefix(folder) {
            if let Some(parent) = relative.parent() {
                hints.extend(
                    parent
                        .components()
                        .filter_map(|component| component.as_os_str().to_str().map(str::to_owned)),
                );
            }
        }
    }
    hints.extend(metadata.ocr_evidence.iter().map(|item| item.text.clone()));
    hints.join(" ")
}

fn lexical_overlap_score(brief_tokens: &HashSet<String>, hints: &str) -> usize {
    lexical_tokens(hints).intersection(brief_tokens).count()
}

fn rank_visual_batches(mut batches: Vec<VisualBatchRanking>) -> Vec<VisualBatchRanking> {
    batches.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.created_at.cmp(&right.created_at))
            .then_with(|| left.task_id.cmp(&right.task_id))
    });
    batches
}

pub(crate) fn prioritize_pending_visual_batches(
    app: &AppHandle,
    project_id: &str,
    brief: &str,
) -> Result<Option<String>, String> {
    let connection = open_connection(app)?;
    let brief_tokens = lexical_tokens(brief);
    let mut asset_scores = HashMap::new();
    let mut assets = connection
        .prepare(
            "SELECT id, display_name, source_reference, folder_reference, metadata_json FROM assets WHERE id IN (SELECT asset_id FROM project_asset_access WHERE project_id = ?1)",
        )
        .map_err(|error| error.to_string())?;
    let rows = assets
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(assets);
    for (asset_id, display_name, source_reference, folder_reference, metadata_json) in rows {
        let metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
        let hints = visual_asset_ranking_text(
            &display_name,
            &source_reference,
            folder_reference.as_deref(),
            &metadata,
        );
        asset_scores.insert(asset_id, lexical_overlap_score(&brief_tokens, &hints));
    }

    let mut tasks = connection
        .prepare(
            "SELECT id, input_json, created_at, status FROM agent_tasks WHERE project_id = ?1 AND tool_name = 'analyze_asset_visual_batch' AND status IN ('queued', 'running') ORDER BY created_at ASC, id ASC",
        )
        .map_err(|error| error.to_string())?;
    let queued = tasks
        .query_map(params![project_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(tasks);
    let rankings_with_status = queued
        .into_iter()
        .map(|(task_id, input_json, created_at, status)| {
            let scores = serde_json::from_str::<Value>(&input_json)
                .ok()
                .and_then(|value| value.get("assetIds").and_then(Value::as_array).cloned())
                .unwrap_or_default()
                .into_iter()
                .filter_map(|id| id.as_str().and_then(|id| asset_scores.get(id)).copied())
                .collect::<Vec<_>>();
            let priority = if scores.is_empty() {
                0
            } else {
                scores.iter().sum::<usize>() / scores.len()
            };
            (
                VisualBatchRanking {
                    task_id,
                    created_at,
                    priority,
                },
                status,
            )
        })
        .collect::<Vec<_>>();
    let highest_running = rankings_with_status
        .iter()
        .filter(|(_, status)| status == "running")
        .map(|(ranking, _)| &ranking.task_id)
        .next()
        .cloned();
    let rankings = rankings_with_status
        .into_iter()
        .map(|(ranking, _)| ranking)
        .collect::<Vec<_>>();
    let ranked = rank_visual_batches(rankings);
    drop(connection);
    let connection = open_connection(app)?;
    for ranking in &ranked {
        connection
            .execute(
                "UPDATE agent_tasks SET result_json = json_set(coalesce(result_json, '{}'), '$.priority', ?1), updated_at = ?2 WHERE id = ?3",
                params![ranking.priority as i64, now_millis(), ranking.task_id],
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(highest_running.or_else(|| ranked.first().map(|r| r.task_id.clone())))
}

fn visual_model_content(
    frames: &[(String, String, Option<i64>, Option<i64>, Vec<u8>)],
) -> Vec<Value> {
    let mut content = vec![
        serde_json::json!({ "type": "input_text", "text": "Look at each frame and say what this shot is. Return JSON {assets:[{assetId,segmentId,timeMs,narrativeRole,caption,subjects,scene,actions,products,qualityNotes}]}. narrativeRole: in your own words, the story job this shot could do in an edited video; invent the phrasing. caption: one short sentence of only what is visible. subjects, actions, products: short visible words. scene: a short place phrase. qualityNotes: only if the frame is hard to use. Match assetId and segmentId to a supplied label. Extra fields are ignored. Empty fields are allowed. Do not infer facts not visible." }),
    ];
    for (asset_id, segment_id, time_ms, duration_ms, image) in frames {
        let mut label = format!(
            "assetId={asset_id}; segmentId={segment_id}; sourceTimeMs={}",
            time_ms.map_or_else(|| "image".to_owned(), |value| value.to_string())
        );
        if let Some(position) = source_position_hint(*time_ms, *duration_ms) {
            label.push_str(&format!("; sourcePosition={position} (hint only)"));
        }
        content.push(serde_json::json!({ "type": "input_text", "text": label }));
        content.push(serde_json::json!({ "type": "input_image", "image_url": format!("data:image/jpeg;base64,{}", STANDARD.encode(image)) }));
    }
    content
}

pub(crate) fn queue_visual_analysis_batch(
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
    let mut visual_units = Vec::new();
    let mut project_id = None;
    for asset_id in asset_ids {
        let row = transaction
            .query_row(
                "SELECT project_id, kind, metadata_json FROM assets WHERE id = ?1 AND analysis_status = 'ready'",
                params![asset_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
            )
            .optional()
            .map_err(|error| error.to_string())?;
        let Some((row_project_id, kind, metadata_json)) = row else {
            continue;
        };
        let mut metadata: TechnicalMetadata =
            serde_json::from_str(&metadata_json).unwrap_or_default();
        if metadata.analysis_cancelled || metadata.library_removed {
            continue;
        }
        project_id.get_or_insert(row_project_id);
        let units = collect_visual_units(asset_id, &kind, &metadata);
        if units.is_empty() {
            metadata.visual_analysis_status = "skipped".to_owned();
            metadata.visual_analysis_note = Some("visual_analysis_not_applicable".to_owned());
        } else {
            metadata.visual_analysis_status = "queued".to_owned();
            metadata.visual_analysis_note = None;
            visual_units.extend(units);
        }
        transaction
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
    if let Some(project_id) = project_id.as_ref().filter(|_| !visual_units.is_empty()) {
        // Worker 只接受 <= VISUAL_ANALYSIS_BATCH_SIZE 张图；按段拆批，不按素材条数。
        for batch in visual_units.chunks(VISUAL_ANALYSIS_BATCH_SIZE) {
            transaction.execute(
                "INSERT INTO agent_tasks (id, project_id, tool_name, status, input_json, result_json, created_at, updated_at) VALUES (?1, ?2, 'analyze_asset_visual_batch', 'queued', ?3, ?4, ?5, ?5)",
                params![
                    Uuid::new_v4().to_string(),
                    project_id,
                    coarse_visual_task_payload(batch).to_string(),
                    serde_json::json!({ "requestedCount": batch.len(), "readyCount": 0, "skippedCount": 0, "failedCount": 0 }).to_string(),
                    now_millis(),
                ],
            ).map_err(|error| error.to_string())?;
        }
    }
    transaction.commit().map_err(|error| error.to_string())?;
    if let Some(project_id) = project_id {
        let _ = app.emit("assets-changed", project_id);
    }
    spawn_visual_analysis_worker(app.clone());
    Ok(())
}

fn run_visual_analysis_batch(app: AppHandle, task_id: String, input_json: String) {
    if !super::controls::task_running(&app, &task_id) {
        return;
    }
    let Some(specs) = serde_json::from_str::<Value>(&input_json)
        .ok()
        .and_then(|value| parse_coarse_visual_specs(&value))
    else {
        let _ = update_visual_batch_task(
            &app,
            &task_id,
            "failed",
            0,
            0,
            0,
            0,
            Some("visual_task_input_invalid"),
        );
        return;
    };
    let requested_count = specs.len();
    let mut asset_ids = Vec::new();
    let mut seen_assets = HashSet::new();
    for (asset_id, _) in &specs {
        if seen_assets.insert(asset_id.clone()) {
            asset_ids.push(asset_id.clone());
        }
    }
    let _ = update_visual_batch_task(&app, &task_id, "running", requested_count, 0, 0, 0, None);
    let _ = update_visual_metadata(
        &app,
        Some(&task_id),
        &asset_ids,
        "running",
        &HashMap::new(),
        None,
    );

    let assets = (|| -> Result<HashMap<String, (String, TechnicalMetadata)>, &'static str> {
        let connection = open_connection(&app).map_err(|_| "visual_storage_failed")?;
        let mut map = HashMap::new();
        for asset_id in &asset_ids {
            let (kind, metadata): (String, TechnicalMetadata) = connection
                .query_row(
                    "SELECT kind, metadata_json FROM assets WHERE id = ?1 AND analysis_status = 'ready'",
                    params![asset_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            serde_json::from_str(&row.get::<_, String>(1)?).unwrap_or_else(|e| {
                                log::warn!("Asset metadata_json could not be parsed: {e}");
                                Default::default()
                            }),
                        ))
                    },
                )
                .map_err(|_| "visual_asset_unavailable")?;
            if !metadata.analysis_cancelled && !metadata.library_removed {
                map.insert(asset_id.clone(), (kind, metadata));
            }
        }
        Ok(map)
    })();
    let Ok(assets) = assets else {
        fail_or_retry_visual_batch(
            &app,
            &task_id,
            &asset_ids,
            requested_count,
            "visual_asset_unavailable",
        );
        return;
    };

    let mut expected: HashMap<String, Option<i64>> = HashMap::new();
    let mut frames = Vec::new();
    let mut kind_by_asset = HashMap::new();
    for (asset_id, segment_id) in &specs {
        let Some((kind, metadata)) = assets.get(asset_id) else {
            continue;
        };
        kind_by_asset.insert(asset_id.clone(), kind.clone());
        let units = collect_visual_units(asset_id, kind, metadata);
        let unit = if segment_id.is_empty() {
            units.into_iter().next()
        } else {
            units
                .into_iter()
                .find(|unit| unit.segment_id == *segment_id)
        };
        let Some(unit) = unit else {
            continue;
        };
        expected.insert(unit_key(asset_id, &unit.segment_id), unit.time_ms);
        let Ok(image) = fs::read(&unit.image_path) else {
            fail_or_retry_visual_batch(
                &app,
                &task_id,
                &asset_ids,
                requested_count,
                "visual_frame_unavailable",
            );
            return;
        };
        frames.push((
            unit.asset_id,
            unit.segment_id,
            unit.time_ms,
            metadata.duration_ms,
            image,
        ));
    }
    if frames.is_empty() {
        fail_or_retry_visual_batch(
            &app,
            &task_id,
            &asset_ids,
            requested_count,
            "visual_frame_unavailable",
        );
        return;
    }
    if !super::controls::task_running(&app, &task_id) {
        return;
    }
    let content = visual_model_content(&frames);
    let access = match ModelAccess::resolve() {
        Ok(access) => access,
        Err(error) => {
            log::warn!("Visual analysis batch: provider access failed: {error}.");
            fail_or_retry_visual_batch(
                &app,
                &task_id,
                &asset_ids,
                requested_count,
                "visual_provider_unavailable",
            );
            return;
        }
    };
    let request = serde_json::json!({ "model": "gpt-5.4", "store": false, "stream": true, "input": [{ "role": "user", "content": content }], "text": { "format": { "type": "json_object" } } });
    let mut failure_note = "visual_response_invalid".to_owned();
    let response_body =
        match post_visual_model_payload(&access, &request, Some(VISUAL_ANALYSIS_TIMEOUT)) {
            Ok(body) => body,
            Err(error) if error == "visual_provider_circuit_open" => {
                let _ = update_visual_metadata(
                    &app,
                    Some(&task_id),
                    &asset_ids,
                    "queued",
                    &HashMap::new(),
                    Some("visual_provider_cooldown"),
                );
                let _ = update_visual_batch_task(
                    &app,
                    &task_id,
                    "queued",
                    requested_count,
                    0,
                    0,
                    0,
                    Some("visual_provider_cooldown"),
                );
                return;
            }
            Err(error) => {
                log::warn!("Visual model request failed: {error}");
                failure_note = crate::provider::classify_model_request_failure(&error).code;
                String::new()
            }
        };
    let response = (!response_body.is_empty())
        .then_some(response_body)
        .and_then(|body| model_response_json_text(&access, &body))
        .and_then(|text| serde_json::from_str::<VisualBatchResponse>(&text).ok());
    let Some(response) = response else {
        complete_visual_model_request(false);
        if !super::controls::task_running(&app, &task_id) {
            return;
        }
        fail_or_retry_visual_batch(&app, &task_id, &asset_ids, requested_count, &failure_note);
        return;
    };
    let mut cards_by_asset: HashMap<String, Vec<VisualEvidence>> = HashMap::new();
    let mut matched = HashSet::new();
    for value in response.assets {
        let Ok(item) = serde_json::from_value::<VisualBatchAsset>(value) else {
            continue;
        };
        let Some((key, time_ms)) = bind_coarse_visual_key(&item, &expected) else {
            continue;
        };
        if !matched.insert(key.clone()) {
            continue;
        }
        cards_by_asset
            .entry(item.asset_id.clone())
            .or_default()
            .push(coarse_visual_card(&item, unit_key_segment(&key), time_ms));
    }
    if cards_by_asset.is_empty() {
        complete_visual_model_request(false);
        if !super::controls::task_running(&app, &task_id) {
            return;
        }
        fail_or_retry_visual_batch(
            &app,
            &task_id,
            &asset_ids,
            requested_count,
            "visual_response_invalid",
        );
        return;
    }
    complete_visual_model_request(true);
    if !super::controls::task_running(&app, &task_id) {
        return;
    }
    let mut failed_ids = Vec::new();
    for (asset_id, segment_id) in &specs {
        if cards_by_asset.contains_key(asset_id) {
            continue;
        }
        let resolved_segment = if segment_id.is_empty() {
            frames
                .iter()
                .find(|(id, _, _, _, _)| id == asset_id)
                .map(|(_, segment_id, _, _, _)| segment_id.as_str())
                .unwrap_or("")
        } else {
            segment_id.as_str()
        };
        if !matched.contains(&unit_key(asset_id, resolved_segment))
            && !failed_ids.iter().any(|id| id == asset_id)
        {
            failed_ids.push(asset_id.clone());
        }
    }
    if commit_coarse_visual_cards(&app, &task_id, &kind_by_asset, &cards_by_asset, &failed_ids)
        .is_err()
    {
        fail_or_retry_visual_batch(
            &app,
            &task_id,
            &asset_ids,
            requested_count,
            "visual_metadata_conflict",
        );
        return;
    }
    let ready_count = asset_ids
        .iter()
        .filter(|asset_id| !failed_ids.iter().any(|id| id == *asset_id))
        .count();
    let _ = update_visual_batch_task(
        &app,
        &task_id,
        "completed",
        requested_count,
        ready_count,
        0,
        failed_ids.len(),
        (!failed_ids.is_empty()).then_some("visual_response_incomplete"),
    );
}

pub(crate) fn spawn_visual_analysis_worker(app: AppHandle) {
    if VISUAL_ANALYSIS_WORKER_ACTIVE
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        loop {
            if visual_model_retry_after().is_some() {
                break;
            }
            let task = (|| -> Result<Option<(String, String, Vec<String>, String)>, String> {
                let connection = open_connection(&app)?;
                let transaction = connection
                    .unchecked_transaction()
                    .map_err(|error| error.to_string())?;
                let row = transaction.query_row(
                    "SELECT id, tool_name, input_json FROM agent_tasks WHERE tool_name IN ('analyze_asset_visual_batch', 'analyze_asset_segments_batch') AND status = 'queued' ORDER BY COALESCE(json_extract(result_json, '$.priority'), 0) DESC, created_at ASC, id ASC LIMIT 1",
                    [],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
                ).optional().map_err(|error| error.to_string())?;
                let Some((task_id, tool_name, input_json)) = row else {
                    return Ok(None);
                };
                let parsed = serde_json::from_str::<Value>(&input_json).ok();
                let asset_ids = if tool_name == "analyze_asset_segments_batch" {
                    parsed
                        .as_ref()
                        .and_then(parse_asset_ids)
                        .filter(|ids| !ids.is_empty() && ids.len() <= VISUAL_ANALYSIS_BATCH_SIZE)
                } else {
                    parsed
                        .as_ref()
                        .and_then(parse_coarse_visual_specs)
                        .map(|specs| {
                            let mut ids = Vec::new();
                            let mut seen = HashSet::new();
                            for (asset_id, _) in specs {
                                if seen.insert(asset_id.clone()) {
                                    ids.push(asset_id);
                                }
                            }
                            ids
                        })
                        .filter(|ids| !ids.is_empty())
                };
                let Some(asset_ids) = asset_ids else {
                    update_visual_batch_task(
                        &app,
                        &task_id,
                        "failed",
                        0,
                        0,
                        0,
                        0,
                        Some("visual_task_input_invalid"),
                    )?;
                    return Ok(Some((task_id, tool_name, Vec::new(), input_json)));
                };
                let claimed = transaction.execute(
                    "UPDATE agent_tasks SET status = 'running', updated_at = ?1 WHERE id = ?2 AND status = 'queued'",
                    params![now_millis(), task_id],
                ).map_err(|error| error.to_string())?;
                transaction.commit().map_err(|error| error.to_string())?;
                if claimed == 1 {
                    Ok(Some((task_id, tool_name, asset_ids, input_json)))
                } else {
                    Ok(Some((task_id, tool_name, Vec::new(), input_json)))
                }
            })().inspect_err(|e| log::warn!("Visual analysis worker: task claim failed: {e}"));
            match task {
                Ok(Some((task_id, tool_name, asset_ids, input_json))) => {
                    if tool_name == "analyze_asset_segments_batch" {
                        if !asset_ids.is_empty() {
                            super::segment_visual::run_segment_visual_analysis_batch(
                                app.clone(),
                                task_id,
                                asset_ids,
                            );
                        }
                    } else if !asset_ids.is_empty() {
                        run_visual_analysis_batch(app.clone(), task_id, input_json);
                    }
                }
                Ok(None) => {
                    match super::retry::requeue_retryable_visual_failures(&app) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => continue,
                    }
                }
                Err(_) => break,
            }
        }
        VISUAL_ANALYSIS_WORKER_ACTIVE.store(false, Ordering::Release);
        if let Some(retry_after) = visual_model_retry_after() {
            schedule_visual_analysis_wake(app.clone(), retry_after);
            return;
        }
        let has_pending = open_connection(&app)
            .ok()
            .and_then(|connection| {
                connection
                    .query_row(
                        "SELECT COUNT(*) FROM agent_tasks WHERE tool_name IN ('analyze_asset_visual_batch', 'analyze_asset_segments_batch') AND status = 'queued'",
                        [],
                        |row| row.get::<_, i64>(0),
                    )
                    .ok()
                    .map(|count| count > 0)
            })
            .unwrap_or(false);
        if has_pending {
            thread::sleep(Duration::from_millis(250));
            spawn_visual_analysis_worker(app);
        }
    });
}

fn schedule_visual_analysis_wake(app: AppHandle, retry_after: Duration) {
    if VISUAL_ANALYSIS_WAKE_SCHEDULED
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        thread::sleep(retry_after + Duration::from_millis(100));
        VISUAL_ANALYSIS_WAKE_SCHEDULED.store(false, Ordering::Release);
        spawn_visual_analysis_worker(app);
    });
}

pub(crate) fn recover_interrupted_visual_batches(app: &AppHandle) -> Result<(), String> {
    log::info!("[PERF] recover_interrupted_visual_batches: starting");
    let start = std::time::Instant::now();
    let connection = open_connection(app)?;
    // 片段任务不修改整素材视觉状态，也不清除已缓存的片段证据。
    connection.execute(
        "UPDATE agent_tasks SET status = 'queued', error_message = NULL, updated_at = ?1 WHERE tool_name = 'analyze_asset_segments_batch' AND status = 'running'",
        params![now_millis()],
    ).map_err(|error| error.to_string())?;
    let rows = connection
        .prepare(
            "SELECT id, input_json FROM agent_tasks WHERE tool_name = 'analyze_asset_visual_batch' AND status = 'running'",
        )
        .map_err(|error| error.to_string())?
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    log::info!(
        "[PERF] recover_interrupted_visual_batches: found {} interrupted batches",
        rows.len()
    );
    let mut asset_ids = Vec::new();
    let mut invalid_asset_ids = Vec::new();
    for (task_id, input_json) in rows {
        let parsed = serde_json::from_str::<Value>(&input_json).ok();
        if let Some(specs) = parsed.as_ref().and_then(parse_coarse_visual_specs) {
            connection
                .execute(
                    "UPDATE agent_tasks SET status = 'queued', error_message = NULL, updated_at = ?1 WHERE id = ?2",
                    params![now_millis(), task_id],
                )
                .map_err(|error| error.to_string())?;
            for (asset_id, _) in specs {
                asset_ids.push(asset_id);
            }
        } else {
            if let Some(ids) = parsed.as_ref().and_then(parse_asset_ids) {
                invalid_asset_ids.extend(ids);
            }
            update_visual_batch_task(
                app,
                &task_id,
                "failed",
                0,
                0,
                0,
                0,
                Some("visual_task_input_invalid"),
            )?;
        }
    }
    drop(connection);
    if !invalid_asset_ids.is_empty() {
        update_visual_metadata(
            app,
            None,
            &invalid_asset_ids,
            "failed",
            &HashMap::new(),
            Some("visual_task_input_invalid"),
        )?;
    }
    if !asset_ids.is_empty() {
        update_visual_metadata(app, None, &asset_ids, "queued", &HashMap::new(), None)?;
    }
    log::info!(
        "[PERF] recover_interrupted_visual_batches: total time {:?}",
        start.elapsed()
    );
    Ok(())
}

pub(crate) fn backfill_queued_visual_batches(app: &AppHandle) -> Result<(), String> {
    log::info!("[PERF] backfill_queued_visual_batches: starting");
    let start = std::time::Instant::now();
    let connection = open_connection(app)?;
    let mut active_ids = HashSet::new();

    let step_start = std::time::Instant::now();
    let mut tasks = connection
        .prepare("SELECT input_json FROM agent_tasks WHERE tool_name = 'analyze_asset_visual_batch' AND status IN ('queued', 'running')")
        .map_err(|error| error.to_string())?;
    let active_rows = tasks
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(tasks);
    log::info!(
        "[PERF] backfill_queued_visual_batches: query active batches took {:?}, found {} batches",
        step_start.elapsed(),
        active_rows.len()
    );

    for input_json in active_rows {
        if let Some(ids) = serde_json::from_str::<serde_json::Value>(&input_json)
            .ok()
            .and_then(|value| {
                value
                    .get("assetIds")
                    .and_then(serde_json::Value::as_array)
                    .cloned()
            })
        {
            active_ids.extend(ids.iter().filter_map(|id| id.as_str().map(str::to_owned)));
        }
    }
    log::info!(
        "[PERF] backfill_queued_visual_batches: collected {} active asset IDs",
        active_ids.len()
    );

    let step_start = std::time::Instant::now();
    let mut statement = connection
        .prepare("SELECT id, project_id, kind, metadata_json FROM assets WHERE analysis_status = 'ready'")
        .map_err(|error| error.to_string())?;
    let rows = statement
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
    drop(statement);
    drop(connection);
    log::info!(
        "[PERF] backfill_queued_visual_batches: query ready assets took {:?}, found {} assets",
        step_start.elapsed(),
        rows.len()
    );

    let step_start = std::time::Instant::now();
    let mut by_project = HashMap::<String, Vec<String>>::new();
    for (asset_id, project_id, kind, metadata_json) in rows {
        let metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
        if !metadata.analysis_cancelled
            && !metadata.library_removed
            && metadata.visual_analysis_status == "queued"
            && !active_ids.contains(&asset_id)
            && !collect_visual_units(&asset_id, &kind, &metadata).is_empty()
        {
            by_project.entry(project_id).or_default().push(asset_id);
        }
    }
    let orphan_count: usize = by_project.values().map(|v| v.len()).sum();
    log::info!(
        "[PERF] backfill_queued_visual_batches: identified {} orphan assets needing batches",
        orphan_count
    );

    let mut created_batches = 0;
    for asset_ids in by_project.into_values() {
        for batch in asset_ids.chunks(VISUAL_ANALYSIS_BATCH_SIZE) {
            queue_visual_analysis_batch(app, batch)?;
            created_batches += 1;
        }
    }
    log::info!(
        "[PERF] backfill_queued_visual_batches: created {} new batches, took {:?}",
        created_batches,
        step_start.elapsed()
    );
    log::info!(
        "[PERF] backfill_queued_visual_batches: total time {:?}",
        start.elapsed()
    );
    Ok(())
}

/// 用户主动跳过视觉分析；已 skipped 的素材不被在途批次覆盖。
#[tauri::command(async)]
pub fn skip_asset_visual_analysis_batch(
    app: AppHandle,
    project_id: String,
    asset_ids: Vec<String>,
) -> Result<BatchAssetActionResult, String> {
    if asset_ids.is_empty() {
        return Err("Select one or more imported assets to skip visual analysis.".to_owned());
    }
    if asset_ids.len() > 200 {
        return Err("Select no more than 200 assets for one batch action.".to_owned());
    }
    let requested_count = asset_ids.len();
    let connection = open_connection(&app)?;
    let timestamp = now_millis();
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    let mut updated_count = 0usize;
    for asset_id in &asset_ids {
        let row = transaction.query_row(
            "SELECT kind, analysis_status, metadata_json FROM assets WHERE id = ?1 AND id IN (SELECT asset_id FROM project_asset_access WHERE project_id = ?2)",
            params![asset_id, project_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        ).optional().map_err(|error| error.to_string())?;
        let Some((kind, analysis_status, metadata_json)) = row else {
            return Err("Selected asset is not available in this project.".to_owned());
        };
        if analysis_status != "ready" || !matches!(kind.as_str(), "video" | "image") {
            continue;
        }
        let mut metadata: TechnicalMetadata =
            serde_json::from_str(&metadata_json).unwrap_or_default();
        if metadata.visual_analysis_status == "skipped" {
            continue;
        }
        metadata.visual_analysis_status = "skipped".to_owned();
        metadata.visual_analysis_note = Some("visual_analysis_skipped_by_user".to_owned());
        metadata.visual_evidence.clear();
        for segment in &mut metadata.scene_segments {
            segment.visual_evidence = None;
        }
        transaction.execute(
            "UPDATE assets SET metadata_json = ?1, updated_at = ?2 WHERE id = ?3 AND project_id = ?4",
            params![serde_json::to_string(&metadata).map_err(|error| error.to_string())?, timestamp, asset_id, project_id],
        ).map_err(|error| error.to_string())?;
        updated_count += 1;
    }
    transaction.execute(
        "INSERT INTO operation_logs (id, project_id, actor, operation_type, entity_type, entity_id, after_json, created_at) VALUES (?1, ?2, 'user', 'skip_asset_visual_analysis_batch', 'project_assets', ?2, ?3, ?4)",
        params![Uuid::new_v4().to_string(), project_id, serde_json::json!({ "requestedCount": requested_count, "skippedCount": updated_count }).to_string(), timestamp],
    ).map_err(|error| error.to_string())?;
    transaction.commit().map_err(|error| error.to_string())?;
    Ok(BatchAssetActionResult {
        requested_count,
        updated_count,
        skipped_count: requested_count.saturating_sub(updated_count),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_scoring_handles_ascii_and_cjk_and_preserves_fifo_ties() {
        let brief = lexical_tokens("Coffee launch 上海咖啡");
        assert!(lexical_overlap_score(&brief, "coffee product") > 0);
        assert!(lexical_overlap_score(&brief, "上海街景 咖啡店") > 0);
        assert_eq!(lexical_overlap_score(&brief, "mountain hiking"), 0);

        let ranked = rank_visual_batches(vec![
            VisualBatchRanking {
                task_id: "first".to_owned(),
                created_at: 10,
                priority: 0,
            },
            VisualBatchRanking {
                task_id: "second".to_owned(),
                created_at: 20,
                priority: 0,
            },
            VisualBatchRanking {
                task_id: "relevant".to_owned(),
                created_at: 30,
                priority: 1,
            },
        ]);
        assert_eq!(
            ranked
                .iter()
                .map(|batch| batch.task_id.as_str())
                .collect::<Vec<_>>(),
            vec!["relevant", "first", "second"]
        );
    }

    #[test]
    fn visual_provider_payload_contains_no_local_path_hints() {
        let local_path = r"D:\private\客户项目\coffee-launch.mp4";
        let content = visual_model_content(&[(
            "asset-1".to_owned(),
            "s001".to_owned(),
            Some(1200),
            Some(10_000),
            vec![1, 2, 3],
        )]);
        let payload = serde_json::to_string(&content).expect("visual content should serialize");

        assert!(!payload.contains(local_path));
        assert!(!payload.contains("coffee-launch.mp4"));
        assert!(payload.contains("asset-1"));
        assert!(payload.contains("segmentId=s001"));
        assert!(payload.contains("sourceTimeMs=1200"));
        assert!(payload.contains("narrativeRole"));
        assert!(payload.contains("sourcePosition=about 12% of the source"));
        assert!(!payload.contains("establishing"));
    }

    #[test]
    fn an_inflight_visual_batch_cannot_overwrite_an_explicit_user_skip() {
        let explicit_skip = TechnicalMetadata {
            visual_analysis_status: "skipped".to_owned(),
            visual_analysis_note: Some("visual_analysis_skipped_by_user".to_owned()),
            ..TechnicalMetadata::default()
        };
        assert!(preserves_explicit_visual_skip(&explicit_skip, "ready"));
        let automatic_skip = TechnicalMetadata {
            visual_analysis_status: "skipped".to_owned(),
            visual_analysis_note: Some("visual_analysis_not_applicable".to_owned()),
            ..TechnicalMetadata::default()
        };
        assert!(!preserves_explicit_visual_skip(&automatic_skip, "queued"));
    }

    fn segment_with_frames(id: &str, times: &[i64]) -> crate::models::SceneSegment {
        crate::models::SceneSegment {
            id: id.to_owned(),
            start_ms: times.first().copied().unwrap_or(0),
            end_ms: times.last().copied().unwrap_or(0) + 1000,
            scene_duration_ms: None,
            visual_quality_score: None,
            frames: times
                .iter()
                .map(|time_ms| crate::models::KeyframeMetadata {
                    time_ms: *time_ms,
                    image_path: format!("{id}-{time_ms}.jpg"),
                })
                .collect(),
            visual_evidence: None,
            motion_score: None,
            motion_profile: None,
        }
    }

    #[test]
    fn collect_visual_units_sends_one_midpoint_per_hard_cut_segment() {
        let metadata = TechnicalMetadata {
            scene_segments: vec![
                segment_with_frames("s001", &[100, 400, 800]),
                segment_with_frames("s002", &[2000, 2600]),
            ],
            ..TechnicalMetadata::default()
        };
        let units = collect_visual_units("asset-1", "video", &metadata);
        assert_eq!(
            units,
            vec![
                CoarseVisualUnit {
                    asset_id: "asset-1".to_owned(),
                    segment_id: "s001".to_owned(),
                    time_ms: Some(400),
                    image_path: "s001-400.jpg".to_owned(),
                },
                CoarseVisualUnit {
                    asset_id: "asset-1".to_owned(),
                    segment_id: "s002".to_owned(),
                    time_ms: Some(2600),
                    image_path: "s002-2600.jpg".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn coarse_visual_tasks_chunk_eight_segments_into_two_batches() {
        let units: Vec<_> = (1..=8)
            .map(|index| CoarseVisualUnit {
                asset_id: "asset-1".to_owned(),
                segment_id: format!("s{index:03}"),
                time_ms: Some(index as i64 * 1000),
                image_path: format!("s{index:03}.jpg"),
            })
            .collect();
        let batches: Vec<_> = units.chunks(VISUAL_ANALYSIS_BATCH_SIZE).collect();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 6);
        assert_eq!(batches[1].len(), 2);
        let first = coarse_visual_task_payload(batches[0]);
        assert_eq!(first["assetIds"], serde_json::json!(["asset-1"]));
        assert_eq!(
            first["segments"].as_array().map(|items| items.len()),
            Some(6)
        );
        assert_eq!(first["segments"][0]["segmentId"], "s001");
    }

    #[test]
    fn parse_coarse_visual_specs_accepts_remainder_of_four() {
        let specs = parse_coarse_visual_specs(&serde_json::json!({
            "assetIds": ["a"],
            "segments": [
                {"assetId": "a", "segmentId": "s001"},
                {"assetId": "a", "segmentId": "s002"},
                {"assetId": "a", "segmentId": "s003"},
                {"assetId": "a", "segmentId": "s004"}
            ]
        }))
        .expect("remainder batch of four");
        assert_eq!(specs.len(), 4);
        assert_eq!(specs[3].1, "s004");
    }

    #[test]
    fn parse_coarse_visual_specs_accepts_legacy_asset_id_batches() {
        let specs = parse_coarse_visual_specs(&serde_json::json!({
            "assetIds": ["a", "b"]
        }))
        .expect("legacy payload");
        assert_eq!(
            specs,
            vec![
                ("a".to_owned(), String::new()),
                ("b".to_owned(), String::new())
            ]
        );
    }

    #[test]
    fn coarse_visual_keeps_matching_card_when_one_identity_is_wrong() {
        let mut expected = HashMap::new();
        expected.insert(unit_key("asset-1", "s001"), Some(400));
        expected.insert(unit_key("asset-1", "s002"), Some(2600));
        let good = serde_json::from_value::<VisualBatchAsset>(serde_json::json!({
            "assetId": "asset-1",
            "segmentId": "s001",
            "timeMs": 999,
            "narrativeRole": "车间开场",
            "caption": "工人站在产线旁",
            "unknownField": "ignored"
        }))
        .expect("matching card");
        let bad = serde_json::from_value::<VisualBatchAsset>(serde_json::json!({
            "assetId": "other",
            "segmentId": "s009",
            "timeMs": 1
        }))
        .expect("unmatched card");
        assert_eq!(
            bind_coarse_visual_key(&good, &expected),
            Some((unit_key("asset-1", "s001"), Some(400)))
        );
        assert_eq!(bind_coarse_visual_key(&bad, &expected), None);
        let card = coarse_visual_card(&good, "s001", Some(400));
        assert_eq!(card.narrative_role.as_deref(), Some("车间开场"));
        assert_eq!(card.caption.as_deref(), Some("工人站在产线旁"));
        assert_eq!(card.time_ms, Some(400));
    }

    #[test]
    fn coarse_visual_response_ignores_unknown_fields() {
        let parsed = serde_json::from_str::<VisualBatchResponse>(
            r#"{"assets":[{"assetId":"asset-1","segmentId":"s001","narrativeRole":"","extra":true}]}"#,
        )
        .expect("extra fields should not fail JSON");
        assert_eq!(parsed.assets.len(), 1);
        let item = serde_json::from_value::<VisualBatchAsset>(parsed.assets[0].clone())
            .expect("one asset card");
        assert_eq!(item.asset_id, "asset-1");
        assert_eq!(item.segment_id.as_deref(), Some("s001"));
    }
}
