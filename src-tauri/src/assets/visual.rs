//! 视觉分析批次队列：远端视觉模型请求、优先级排序、批次worker与恢复。
//! 技术分析完成后自动排队；storyboard brief 可对pending批次重新排序。

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
use tauri::{AppHandle, Manager};
use uuid::Uuid;

const VISUAL_ANALYSIS_TIMEOUT: Duration = Duration::from_secs(30);
const PRIORITY_VISUAL_WAIT_TIMEOUT: Duration = Duration::from_secs(65);
pub(crate) const VISUAL_ANALYSIS_BATCH_SIZE: usize = 6;

static VISUAL_ANALYSIS_WORKER_ACTIVE: AtomicBool = AtomicBool::new(false);
static VISUAL_ANALYSIS_WAKE_SCHEDULED: AtomicBool = AtomicBool::new(false);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VisualBatchResponse {
    #[serde(default)]
    assets: Vec<VisualBatchAsset>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct VisualBatchAsset {
    asset_id: String,
    #[serde(default)]
    time_ms: Option<i64>,
    #[serde(default)]
    subjects: Vec<String>,
    #[serde(default)]
    scene: Option<String>,
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default)]
    products: Vec<String>,
    #[serde(default)]
    quality_notes: Vec<String>,
}

#[derive(Clone)]
struct VisualBatchRanking {
    task_id: String,
    created_at: i64,
    priority: usize,
}

// representative_frame 从 analysis 模块导入
use super::analysis::representative_frame;

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
            "UPDATE agent_tasks SET status = ?1, result_json = ?2, error_message = ?3, updated_at = ?4 WHERE id = ?5",
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
    asset_ids: &[String],
    status: &str,
    evidence: &HashMap<String, VisualEvidence>,
    note: Option<&str>,
) -> Result<(), String> {
    let connection = open_connection(app)?;
    let transaction = connection
        .unchecked_transaction()
        .map_err(|error| error.to_string())?;
    for asset_id in asset_ids {
        let metadata_json: String = transaction
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
        if preserves_explicit_visual_skip(&metadata, status) {
            continue;
        }
        metadata.visual_analysis_status = status.to_owned();
        metadata.visual_analysis_note = note.map(str::to_owned);
        if let Some(item) = evidence.get(asset_id) {
            metadata.visual_evidence = vec![item.clone()];
        } else if status == "failed" || status == "skipped" {
            metadata.visual_evidence.clear();
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
            "SELECT id, display_name, source_reference, folder_reference, metadata_json FROM assets WHERE project_id = ?1",
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

pub(crate) fn wait_for_visual_batch(app: &AppHandle, task_id: Option<&str>) -> Result<(), String> {
    let Some(task_id) = task_id else {
        return Ok(());
    };
    let deadline = std::time::Instant::now() + PRIORITY_VISUAL_WAIT_TIMEOUT;
    loop {
        let status: String = open_connection(app)?
            .query_row(
                "SELECT status FROM agent_tasks WHERE id = ?1",
                params![task_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if status == "completed" {
            return Ok(());
        }
        if std::time::Instant::now() >= deadline || !matches!(status.as_str(), "queued" | "running")
        {
            return Err("The priority visual analysis batch did not complete in time.".to_owned());
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn visual_model_content(frames: &[(String, Option<i64>, Vec<u8>)]) -> Vec<Value> {
    let mut content = vec![
        serde_json::json!({ "type": "input_text", "text": "Analyze only visible evidence. Return JSON {assets:[{assetId,timeMs,subjects,scene,actions,products,qualityNotes}]}. Each assetId must be one supplied label. Do not infer facts not visible." }),
    ];
    for (asset_id, time_ms, image) in frames {
        content.push(serde_json::json!({ "type": "input_text", "text": format!("assetId={asset_id}; sourceTimeMs={}", time_ms.map_or("image".to_owned(), |value| value.to_string())) }));
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
    let mut visual_asset_ids = Vec::new();
    let mut skipped_asset_ids = Vec::new();
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
        project_id.get_or_insert(row_project_id);
        if representative_frame(&metadata, &kind).is_some() {
            metadata.visual_analysis_status = "queued".to_owned();
            metadata.visual_analysis_note = None;
            visual_asset_ids.push(asset_id.clone());
        } else {
            metadata.visual_analysis_status = "skipped".to_owned();
            metadata.visual_analysis_note = Some("visual_analysis_not_applicable".to_owned());
            skipped_asset_ids.push(asset_id.clone());
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
    if let Some(project_id) = project_id.filter(|_| !visual_asset_ids.is_empty()) {
        transaction.execute(
            "INSERT INTO agent_tasks (id, project_id, tool_name, status, input_json, result_json, created_at, updated_at) VALUES (?1, ?2, 'analyze_asset_visual_batch', 'queued', ?3, ?4, ?5, ?5)",
            params![
                Uuid::new_v4().to_string(),
                project_id,
                serde_json::json!({ "assetIds": visual_asset_ids }).to_string(),
                serde_json::json!({ "requestedCount": visual_asset_ids.len(), "readyCount": 0, "skippedCount": skipped_asset_ids.len(), "failedCount": 0 }).to_string(),
                now_millis(),
            ],
        ).map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())?;
    spawn_visual_analysis_worker(app.clone());
    Ok(())
}

#[rustfmt::skip]
fn run_visual_analysis_batch(app: AppHandle, task_id: String, asset_ids: Vec<String>) {
    let requested_count = asset_ids.len();
    let _ = update_visual_batch_task(&app, &task_id, "running", requested_count, 0, 0, 0, None);
    let _ = update_visual_metadata(&app, &asset_ids, "running", &HashMap::new(), None);

    let assets = (|| -> Result<Vec<(String, String, TechnicalMetadata)>, &'static str> {
        let connection = open_connection(&app).map_err(|_| "visual_storage_failed")?;
        asset_ids
            .iter()
            .map(|asset_id| {
                connection.query_row(
                    "SELECT id, kind, metadata_json FROM assets WHERE id = ?1 AND analysis_status = 'ready'",
                    params![asset_id],
                    |row| Ok((row.get(0)?, row.get(1)?, serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or_else(|e| { log::warn!("Asset metadata_json could not be parsed: {e}"); Default::default() }))),
                ).map_err(|_| "visual_asset_unavailable")
            })
            .collect()
    })();
    let Ok(assets) = assets else {
        let _ = update_visual_metadata(
            &app,
            &asset_ids,
            "failed",
            &HashMap::new(),
            Some("visual_asset_unavailable"),
        );
        let _ = update_visual_batch_task(
            &app,
            &task_id,
            "failed",
            requested_count,
            0,
            0,
            requested_count,
            Some("visual_asset_unavailable"),
        );
        return;
    };
    let mut source_times = HashMap::new();
    let mut frames = Vec::new();
    for (asset_id, kind, metadata) in &assets {
        let Some((image_path, time_ms)) = representative_frame(metadata, kind) else {
            continue;
        };
        source_times.insert(asset_id.clone(), time_ms);
        let Ok(image) = fs::read(image_path) else {
            let _ = update_visual_metadata(
                &app,
                &asset_ids,
                "failed",
                &HashMap::new(),
                Some("visual_frame_unavailable"),
            );
            let _ = update_visual_batch_task(
                &app,
                &task_id,
                "failed",
                requested_count,
                0,
                0,
                requested_count,
                Some("visual_frame_unavailable"),
            );
            return;
        };
        frames.push((asset_id.clone(), time_ms, image));
    }
    let content = visual_model_content(&frames);
    let access = match ModelAccess::resolve() {
        Ok(access) => access,
        Err(error) => {
            log::warn!("Visual analysis batch: provider access failed: {error}.");
            let _ = update_visual_metadata(
                &app,
                &asset_ids,
                "skipped",
                &HashMap::new(),
                Some("visual_provider_unavailable"),
            );
            let _ = update_visual_batch_task(
                &app,
                &task_id,
                "completed",
                requested_count,
                0,
                requested_count,
                0,
                Some("visual_provider_unavailable"),
            );
            return;
        }
    };
    let request = serde_json::json!({ "model": "gpt-5.4", "store": false, "stream": true, "input": [{ "role": "user", "content": content }], "text": { "format": { "type": "json_object" } } });
    let response_body =
        match post_visual_model_payload(&access, &request, Some(VISUAL_ANALYSIS_TIMEOUT)) {
            Ok(body) => body,
            Err(error) if error == "visual_provider_circuit_open" => {
                let _ = update_visual_metadata(
                    &app,
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
            Err(error) => { log::warn!("Visual model request failed: {error}"); String::new() }
        };
    let response = (!response_body.is_empty())
        .then_some(response_body)
        .and_then(|body| model_response_json_text(&access, &body))
        .and_then(|text| serde_json::from_str::<VisualBatchResponse>(&text).ok());
    let Some(response) = response else {
        complete_visual_model_request(false);
        let _ = update_visual_metadata(
            &app,
            &asset_ids,
            "failed",
            &HashMap::new(),
            Some("visual_request_failed"),
        );
        let _ = update_visual_batch_task(
            &app,
            &task_id,
            "failed",
            requested_count,
            0,
            0,
            requested_count,
            Some("visual_request_failed"),
        );
        return;
    };
    let allowed: HashSet<&str> = asset_ids.iter().map(String::as_str).collect();
    if response.assets.iter().any(|item| {
        !allowed.contains(item.asset_id.as_str())
            || source_times.get(&item.asset_id).copied().flatten() != item.time_ms
    }) {
        complete_visual_model_request(false);
        let _ = update_visual_metadata(
            &app,
            &asset_ids,
            "failed",
            &HashMap::new(),
            Some("visual_response_invalid"),
        );
        let _ = update_visual_batch_task(
            &app,
            &task_id,
            "failed",
            requested_count,
            0,
            0,
            requested_count,
            Some("visual_response_invalid"),
        );
        return;
    };
    complete_visual_model_request(true);
    let mut visual = HashMap::new();
    let mut ready_ids = Vec::new();
    let mut failed_ids = Vec::new();
    for asset_id in &asset_ids {
        let Some(item) = response.assets.iter().find(|item| &item.asset_id == asset_id) else {
            failed_ids.push(asset_id.clone());
            continue;
        };
        ready_ids.push(asset_id.clone());
        visual.insert(
            asset_id.clone(),
            VisualEvidence {
                time_ms: item.time_ms,
                subjects: item.subjects.clone(),
                scene: item.scene.clone(),
                actions: item.actions.clone(),
                products: item.products.clone(),
                quality_notes: item.quality_notes.clone(),
            },
        );
    }
    let _ = update_visual_metadata(&app, &ready_ids, "ready", &visual, None);
    let _ = update_visual_metadata(
        &app,
        &failed_ids,
        "failed",
        &HashMap::new(),
        Some("visual_response_incomplete"),
    );
    let _ = update_visual_batch_task(
        &app,
        &task_id,
        "completed",
        requested_count,
        ready_ids.len(),
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
            let task = (|| -> Result<Option<(String, Vec<String>)>, String> {
                let connection = open_connection(&app)?;
                let transaction = connection
                    .unchecked_transaction()
                    .map_err(|error| error.to_string())?;
                let row = transaction.query_row("SELECT id, input_json FROM agent_tasks WHERE tool_name = 'analyze_asset_visual_batch' AND status = 'queued' ORDER BY COALESCE(json_extract(result_json, '$.priority'), 0) DESC, created_at ASC, id ASC LIMIT 1", [], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))).optional().map_err(|error| error.to_string())?;
                let Some((task_id, input_json)) = row else {
                    return Ok(None);
                };
                let asset_ids = serde_json::from_str::<serde_json::Value>(&input_json)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("assetIds")
                            .and_then(serde_json::Value::as_array)
                            .map(|ids| {
                                ids.iter()
                                    .filter_map(|id| id.as_str().map(str::to_owned))
                                    .collect::<Vec<_>>()
                            })
                    })
                    .filter(|ids| !ids.is_empty() && ids.len() <= VISUAL_ANALYSIS_BATCH_SIZE);
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
                    return Ok(Some((task_id, Vec::new())));
                };
                let claimed = transaction.execute(
                    "UPDATE agent_tasks SET status = 'running', updated_at = ?1 WHERE id = ?2 AND status = 'queued'",
                    params![now_millis(), task_id],
                ).map_err(|error| error.to_string())?;
                transaction.commit().map_err(|error| error.to_string())?;
                if claimed == 1 {
                    Ok(Some((task_id, asset_ids)))
                } else {
                    Ok(Some((task_id, Vec::new())))
                }
            })().inspect_err(|e| log::warn!("Visual analysis worker: task claim failed: {e}"));
            match task {
                Ok(Some((task_id, asset_ids))) => {
                    if !asset_ids.is_empty() {
                        run_visual_analysis_batch(app.clone(), task_id, asset_ids);
                    }
                }
                Ok(None) | Err(_) => break,
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
                        "SELECT COUNT(*) FROM agent_tasks WHERE tool_name = 'analyze_asset_visual_batch' AND status = 'queued'",
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
    let connection = open_connection(app)?;
    let rows = connection
        .prepare(
            "SELECT id, input_json FROM agent_tasks WHERE tool_name = 'analyze_asset_visual_batch' AND status = 'running'",
        )
        .map_err(|error| error.to_string())?
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let mut asset_ids = Vec::new();
    let mut invalid_asset_ids = Vec::new();
    for (task_id, input_json) in rows {
        let parsed = serde_json::from_str::<serde_json::Value>(&input_json)
            .ok()
            .and_then(|value| {
                value
                    .get("assetIds")
                    .and_then(serde_json::Value::as_array)
                    .map(|ids| {
                        ids.iter()
                            .filter_map(|id| id.as_str().map(str::to_owned))
                            .collect::<Vec<_>>()
                    })
            })
            .filter(|ids| !ids.is_empty());
        if let Some(ids) = parsed {
            if ids.len() <= VISUAL_ANALYSIS_BATCH_SIZE {
                connection
                    .execute(
                        "UPDATE agent_tasks SET status = 'queued', error_message = NULL, updated_at = ?1 WHERE id = ?2",
                        params![now_millis(), task_id],
                    )
                    .map_err(|error| error.to_string())?;
                asset_ids.extend(ids);
            } else {
                connection.execute(
                    "UPDATE agent_tasks SET status = 'failed', error_message = 'visual_task_input_invalid', updated_at = ?1 WHERE id = ?2",
                    params![now_millis(), task_id],
                ).map_err(|error| error.to_string())?;
                invalid_asset_ids.extend(ids);
            }
        } else {
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
            &invalid_asset_ids,
            "failed",
            &HashMap::new(),
            Some("visual_task_input_invalid"),
        )?;
    }
    if !asset_ids.is_empty() {
        update_visual_metadata(app, &asset_ids, "queued", &HashMap::new(), None)?;
    }
    Ok(())
}

pub(crate) fn backfill_queued_visual_batches(app: &AppHandle) -> Result<(), String> {
    let connection = open_connection(app)?;
    let mut active_ids = HashSet::new();
    let mut tasks = connection
        .prepare("SELECT input_json FROM agent_tasks WHERE tool_name = 'analyze_asset_visual_batch' AND status IN ('queued', 'running')")
        .map_err(|error| error.to_string())?;
    let active_rows = tasks
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    drop(tasks);
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
    let mut by_project = HashMap::<String, Vec<String>>::new();
    for (asset_id, project_id, kind, metadata_json) in rows {
        let metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
        if metadata.visual_analysis_status == "queued"
            && !active_ids.contains(&asset_id)
            && representative_frame(&metadata, &kind).is_some()
        {
            by_project.entry(project_id).or_default().push(asset_id);
        }
    }
    for asset_ids in by_project.into_values() {
        for batch in asset_ids.chunks(VISUAL_ANALYSIS_BATCH_SIZE) {
            queue_visual_analysis_batch(app, batch)?;
        }
    }
    Ok(())
}

/// 用户主动跳过视觉分析；已 skipped 的素材不被在途批次覆盖。
#[tauri::command]
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
            "SELECT kind, analysis_status, metadata_json FROM assets WHERE id = ?1 AND project_id = ?2",
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
        let content = visual_model_content(&[("asset-1".to_owned(), Some(1200), vec![1, 2, 3])]);
        let payload = serde_json::to_string(&content).expect("visual content should serialize");

        assert!(!payload.contains(local_path));
        assert!(!payload.contains("coffee-launch.mp4"));
        assert!(payload.contains("asset-1"));
        assert!(payload.contains("sourceTimeMs=1200"));
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
}
