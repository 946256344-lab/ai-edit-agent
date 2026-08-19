//! 技能执行与参数校验。
//!
//! `apply_skill` 是唯一合法的副作用入口。所有技能调用必须经过此函数，返回只读观察
//! 结果或更新 `LoopState::last_outcome`。工具白名单、作用域校验和审计记录由调用方
//! (`runtime.rs`) 在调用前后负责。

use crate::jianying::create_jianying_draft;
use crate::models::{
    AgentEditResult, ClipAdjustmentParams, ClipReplacementParams, MusicCue, MusicTrack, TextTrack,
    TimelineVersion,
};
use crate::music_provider::{attribution_for, download_track, eligible_track, search_tracks};
use crate::preview::render_preview;
use crate::timeline::{
    change_clip_duration, create_timeline_draft, reorder_clips, replace_clips,
    replace_music_tracks, replace_text_tracks, select_timeline_candidate, text_recipe_capabilities,
    text_track_quality_warnings, ClipAdjustment, ClipReplacement,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::path::Path;
use tauri::Manager;

use super::schema::LoopState;

// ──────────────────────────────────────────────────────────────────────────────
// 工具产物映射
// ──────────────────────────────────────────────────────────────────────────────

pub(super) fn produced_artifact_for_tool(tool: &str) -> Option<&'static str> {
    match tool {
        "generate_storyboard" => Some("storyboard"),
        "create_timeline_draft"
        | "replace_clips"
        | "change_clip_duration"
        | "reorder_clips"
        | "replace_text_tracks"
        | "replace_music_tracks" => Some("timeline"),
        "render_preview" => Some("preview"),
        "create_jianying_draft" => Some("jianying_draft"),
        _ => None,
    }
}

pub(super) fn persisted_artifact_for_tool(
    state: &LoopState,
    tool: &str,
) -> Option<(&'static str, String)> {
    let result = state.last_outcome.as_ref()?;
    match tool {
        "generate_storyboard" => result
            .storyboard
            .as_ref()
            .map(|artifact| ("storyboard_version", artifact.id.clone())),
        "create_timeline_draft"
        | "replace_clips"
        | "change_clip_duration"
        | "reorder_clips"
        | "replace_text_tracks"
        | "replace_music_tracks" => result
            .timeline
            .as_ref()
            .map(|artifact| ("timeline_version", artifact.id.clone())),
        "render_preview" => result
            .preview
            .as_ref()
            .map(|artifact| ("preview", artifact.timeline_version_id.clone())),
        "create_jianying_draft" => result
            .timeline
            .as_ref()
            .map(|artifact| ("jianying_draft", artifact.id.clone())),
        _ => None,
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// 错误码与诊断上下文
// ──────────────────────────────────────────────────────────────────────────────

pub(super) fn safe_step_error_code(error: &str) -> &'static str {
    if error.starts_with("storyboard_source_inventory_unavailable:")
        || error.starts_with("storyboard_visual_evidence_unavailable:")
    {
        "unavailable_media"
    } else if error.contains("time range") || error.contains("source range") {
        "invalid_source_time_range"
    } else if error.contains("storyboard") || error.contains("timeline") {
        "missing_or_invalid_prerequisite"
    } else if error.contains("asset") || error.contains("media") {
        "unavailable_media"
    } else {
        "skill_execution_failed"
    }
}

fn diagnostic_count(error: &str, key: &str) -> Option<usize> {
    error
        .split([':', ';'])
        .map(str::trim)
        .find_map(|field| field.strip_prefix(&format!("{key}=")))
        .and_then(|value| value.parse().ok())
}

pub(super) fn safe_tool_failure_context(tool: &str, error: &str) -> Value {
    let code = safe_step_error_code(error);

    // 前置条件缺失：render_preview/create_jianying_draft 需要先有 timeline
    if error.starts_with("no_timeline:") {
        return json!({
            "status": "failed",
            "operation": tool,
            "stage": "prerequisite_validation",
            "code": "missing_timeline",
            "facts": ["当前剪辑任务还没有时间线。"],
            "retryable": true,
            "recovery": "请先调用 create_timeline_draft 创建内部时间线，然后再生成预览或剪映草稿。",
            "responseInstruction": "Tell the user they need to create a timeline first (create_timeline_draft), then they can generate preview or Jianying draft. Do not claim the preview or draft was created."
        });
    }

    if error.starts_with("storyboard_source_inventory_unavailable:") {
        let visual_ready = diagnostic_count(error, "visual_ready_candidates").unwrap_or(0);
        let accessible = diagnostic_count(error, "accessible_source_files").unwrap_or(0);
        return json!({
            "status": "failed",
            "operation": tool,
            "stage": "storyboard_source_validation",
            "code": code,
            "facts": [
                format!("{visual_ready} imported assets have completed visual evidence"),
                format!("{accessible} of those source files are currently accessible")
            ],
            "retryable": true,
            "recovery": "Reconnect the source storage or relink the imported media, then retry.",
            "responseInstruction": "Explain the failure naturally in the user's language. State that the requested artifact was not created. Do not invent a more specific filesystem cause and do not claim completion."
        });
    }
    if error.starts_with("storyboard_visual_evidence_unavailable:") {
        return json!({
            "status": "failed",
            "operation": tool,
            "stage": "storyboard_evidence_validation",
            "code": code,
            "facts": ["No imported asset currently has completed visual evidence usable for storyboard generation."],
            "retryable": true,
            "recovery": "Complete or retry visual analysis for relevant imported media, then retry storyboard generation.",
            "responseInstruction": "Explain the failure naturally in the user's language. State that the storyboard was not created. Do not claim source files are missing unless supplied facts say so, and do not claim completion."
        });
    }
    json!({
        "status": "failed",
        "operation": tool,
        "stage": "tool_execution",
        "code": code,
        "facts": ["The local tool rejected the operation before confirming the requested artifact."],
        "retryable": code != "invalid_source_time_range",
        "recovery": "Use the safe code and current task state to explain the failure or choose another allowed tool.",
        "responseInstruction": "Explain only the supplied facts in the user's language. Do not infer hidden details, expose local paths, or claim completion."
    })
}

pub(super) fn safe_failure_explanation(explanation: &str) -> bool {
    let explanation = explanation.trim().to_lowercase();
    !explanation.is_empty()
        && ![
            "已生成",
            "已创建",
            "已完成",
            "生成成功",
            "创建成功",
            "successfully generated",
            "successfully created",
            "completed successfully",
        ]
        .iter()
        .any(|claim| explanation.contains(claim))
}

pub(super) fn should_redirect_storyboard_after_failed_generation(
    goal: super::policy::LoopGoal,
    last_failed_tool_error_code: Option<&str>,
) -> bool {
    goal == super::policy::LoopGoal::Storyboard
        && matches!(last_failed_tool_error_code, Some("skill_execution_failed"))
}

// ──────────────────────────────────────────────────────────────────────────────
// 编辑状态读取
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct EditArtifactState {
    pub(super) has_storyboard: bool,
    pub(super) has_timeline: bool,
    pub(super) has_preview: bool,
}

pub(super) fn artifact_status_message(artifacts: EditArtifactState) -> Option<&'static str> {
    if artifacts.has_preview {
        Some("已经生成可审阅的 local preview。")
    } else if artifacts.has_timeline {
        Some("已经生成内部时间线，但还没有生成 local preview。")
    } else if artifacts.has_storyboard {
        Some("已经生成 storyboard，但还没有生成内部时间线。")
    } else {
        None
    }
}

pub(super) fn edit_status_message(
    previous_status: Option<&str>,
    artifacts: EditArtifactState,
) -> String {
    let artifact_message = artifact_status_message(artifacts);
    match previous_status {
        Some("queued" | "running") => match artifact_message {
            Some(message) => format!("上一条 Agent 任务仍在处理中；{message}"),
            None => "还没剪好，上一条 Agent 任务仍在处理中。".to_owned(),
        },
        Some("needs_clarification") => match artifact_message {
            Some(message) => format!("上一条 Agent 任务正在等待你补充信息；{message}"),
            None => "还没剪好，上一条 Agent 任务正在等待你补充信息。".to_owned(),
        },
        Some("needs_review") => match artifact_message {
            Some(message) => format!("上一条 Agent 任务需要审阅后再继续；{message}"),
            None => "还没确认完成，上一条 Agent 任务需要审阅后再继续。".to_owned(),
        },
        Some("failed") => match artifact_message {
            Some(message) => format!("上一条 Agent 任务没有完成；{message}"),
            None => "还没剪好，上一条 Agent 任务没有完成，也没有把失败当成成功。".to_owned(),
        },
        Some("partially_completed") => match artifact_message {
            Some(message) => format!("上一条 Agent 任务只完成了一部分；{message}"),
            None => "还没完全剪好，上一条 Agent 任务只完成了一部分。".to_owned(),
        },
        Some("completed") | None => artifact_message
            .unwrap_or_else(|| {
                if previous_status.is_some() {
                    "上一条 Agent 请求已完成，但当前没有可检查的剪辑产物。"
                } else {
                    "当前会话还没有可检查的剪辑任务或产物。"
                }
            })
            .to_owned(),
        Some(_) => "当前剪辑状态暂时无法确认。".to_owned(),
    }
}

pub(crate) fn read_scoped_edit_status(
    app: &tauri::AppHandle,
    connection: &Connection,
    project_id: &str,
    editing_task_id: &str,
    conversation_id: &str,
    excluded_agent_task_id: Option<&str>,
) -> Result<String, String> {
    let previous_status = connection
        .query_row(
            "SELECT status FROM agent_tasks WHERE project_id = ?1 AND editing_task_id = ?2 AND conversation_id = ?3 AND (?4 IS NULL OR id != ?4) AND tool_name NOT IN ('analyze_asset', 'analyze_asset_visual_batch', 'get_edit_status') ORDER BY created_at DESC LIMIT 1",
            params![project_id, editing_task_id, conversation_id, excluded_agent_task_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| "Agent edit status could not be read.".to_owned())?;
    let storyboard_id = connection
        .query_row(
            "SELECT id FROM storyboard_versions WHERE project_id = ?1 AND editing_task_id = ?2 ORDER BY version_number DESC, created_at DESC LIMIT 1",
            params![project_id, editing_task_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|_| "Agent storyboard status could not be read.".to_owned())?;
    let latest_timeline = storyboard_id
        .as_deref()
        .map(|storyboard_id| {
            connection
                .query_row(
                    "SELECT id, status FROM timeline_versions WHERE project_id = ?1 AND storyboard_version_id = ?2 ORDER BY version_number DESC, created_at DESC LIMIT 1",
                    params![project_id, storyboard_id],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()
        })
        .transpose()
        .map_err(|_| "Agent timeline status could not be read.".to_owned())?
        .flatten();
    let has_preview = latest_timeline
        .as_ref()
        .is_some_and(|(timeline_id, status)| {
            status == "preview_ready"
                && app
                    .path()
                    .app_data_dir()
                    .ok()
                    .map(|directory| {
                        directory
                            .join("previews")
                            .join(timeline_id)
                            .join("preview.mp4")
                            .is_file()
                    })
                    .unwrap_or(false)
        });
    Ok(edit_status_message(
        previous_status.as_deref(),
        EditArtifactState {
            has_storyboard: storyboard_id.is_some(),
            has_timeline: latest_timeline.is_some(),
            has_preview,
        },
    ))
}

// ──────────────────────────────────────────────────────────────────────────────
// Timeline 辅助
// ──────────────────────────────────────────────────────────────────────────────

pub(super) fn select_timeline_for_tool(
    state: &LoopState,
    args: &Value,
) -> Result<TimelineVersion, String> {
    let timeline_id = args.get("timelineVersionId").and_then(Value::as_str);
    let timeline = select_timeline_candidate(&state.timelines, timeline_id, None).ok_or_else(|| {
        if state.timelines.is_empty() {
            "no_timeline: 当前剪辑任务还没有时间线，请先调用 create_timeline_draft 创建时间线，再生成预览或草稿。".to_owned()
        } else {
            "Agent must select a timeline that belongs to the current storyboard.".to_owned()
        }
    })?;
    if timeline.project_id != state.project_id {
        return Err("timeline_scope_mismatch: 时间线不属于当前项目。".to_owned());
    }
    Ok(timeline)
}

#[rustfmt::skip]
pub(super) fn build_timeline_snapshot(state: &LoopState, requested: Option<&str>) -> Value {
    select_timeline_candidate(&state.timelines, requested, None)
        .map(|timeline| serde_json::to_value(timeline).unwrap_or_else(|e| {
            log::warn!("Timeline could not be serialized for model context: {e}");
            Value::Null
        }))
        .unwrap_or(Value::Null)
}

pub(super) fn upsert(timelines: &mut Vec<TimelineVersion>, updated: TimelineVersion) {
    if let Some(slot) = timelines
        .iter_mut()
        .find(|timeline| timeline.id == updated.id)
    {
        *slot = updated;
    } else {
        timelines.push(updated);
    }
}

pub(super) fn upsert_timeline(timelines: &mut Vec<TimelineVersion>, updated: TimelineVersion) {
    upsert(timelines, updated)
}

// ──────────────────────────────────────────────────────────────────────────────
// 技能执行器
// ──────────────────────────────────────────────────────────────────────────────

/// 在真实且已校验的领域函数上执行一个技能。返回值只作为下一步观察；只有这里落地的
/// 可审计产物才会更新 `last_outcome`，模型文字本身永远不代表副作用成功。
pub(super) fn apply_skill(
    state: &mut LoopState,
    tool: &str,
    args: &Value,
) -> Result<Value, String> {
    let agent_task_id = state.agent_task_id().to_owned();
    match tool {
        "get_edit_status" => {
            let message = read_scoped_edit_status(
                state.app,
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                Some(state.agent_task_id),
            )?;
            state.last_outcome = Some(AgentEditResult {
                agent_task_id: agent_task_id.clone(),
                message: message.clone(),
                storyboard: None,
                timeline: None,
                preview: None,
                jianying_draft: None,
            });
            Ok(json!({"tool":"get_edit_status","status":"ok","message":message}))
        }
        "search_music" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| "search_music needs a query.".to_owned())?;
            let tracks = search_tracks(query)?;
            Ok(json!({"tool":"search_music","status":"ok","tracks":tracks}))
        }
        "download_music" => {
            let track_id = args
                .get("trackId")
                .and_then(Value::as_str)
                .ok_or_else(|| "download_music needs a trackId.".to_owned())?;
            let asset = download_track(&state.app, state.project_id, track_id)?;
            Ok(
                json!({"tool":"download_music","status":"ok","assetId":asset.id,"analysisStatus":asset.analysis_status}),
            )
        }
        "use_online_music" => {
            let track_id = args
                .get("trackId")
                .and_then(Value::as_str)
                .ok_or_else(|| "use_online_music needs a trackId.".to_owned())?;
            let timeline = select_timeline_for_tool(state, args)?;
            let track = eligible_track(track_id)?;
            let attribution = attribution_for(&track);
            let asset = download_track(&state.app, state.project_id, track_id)?;
            let asset =
                crate::assets::wait_for_asset_ready(&state.app, state.project_id, &asset.id)?;
            let timeline_duration = timeline
                .clips
                .iter()
                .map(|clip| clip.timeline_end_ms)
                .max()
                .ok_or_else(|| "Timeline has no clips for music.".to_owned())?;
            let source_duration = asset
                .duration_ms
                .ok_or_else(|| "Music has no verified duration.".to_owned())?;
            let source_end = source_duration.min(timeline_duration);
            let asset_id = asset.id.clone();
            let result = replace_music_tracks(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                &agent_task_id,
                &timeline,
                vec![MusicTrack {
                    id: format!("jamendo-{track_id}"),
                    enabled: true,
                    cues: vec![MusicCue {
                        id: format!("jamendo-{track_id}-cue"),
                        asset_id: asset_id.clone(),
                        source_start_ms: 0,
                        source_end_ms: source_end,
                        timeline_start_ms: 0,
                        timeline_end_ms: timeline_duration,
                        loop_enabled: source_end < timeline_duration,
                        volume: 0.35,
                        fade_in_ms: 250,
                        fade_out_ms: 350,
                        jianying_compatibility: "not_deliverable".to_owned(),
                        provider: Some("Jamendo".to_owned()),
                        license_url: Some(track.license_ccurl),
                        attribution: Some(attribution),
                    }],
                }],
            )?;
            let timeline_version_id = result.id.clone();
            let version_number = result.version_number;
            upsert(&mut state.timelines, result.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message:
                    "已选择并下载一首符合许可条件的背景音乐，完成本地分析后写入新的内部时间线版本。"
                        .to_owned(),
                storyboard: None,
                timeline: Some(result),
                preview: None,
                jianying_draft: None,
            });
            Ok(
                json!({"tool":"use_online_music","status":"ok","timelineVersionId":timeline_version_id,"versionNumber":version_number,"assetId":asset_id}),
            )
        }
        "list_assets" => {
            let assets = crate::assets::list_assets_for_agent(
                state.app.clone(),
                state.project_id.to_owned(),
            )?;
            let summary: Vec<Value> = assets
                .iter()
                .map(|asset| {
                    json!({
                        "id": asset.id,
                        "name": asset.display_name,
                        "kind": asset.kind,
                        "durationMs": asset.duration_ms,
                        "analysisStatus": asset.analysis_status,
                        "sceneCount": asset.scene_count,
                    })
                })
                .collect();
            Ok(json!({ "tool": "list_assets", "status": "ok", "assets": summary }))
        }
        "get_asset_health_summary" => {
            let summary = crate::assets::get_asset_health_summary_for_agent(
                &state.connection,
                state.project_id,
            )?;
            Ok(json!({ "tool": "get_asset_health_summary", "status": "ok", "summary": summary }))
        }
        "search_assets" => {
            let results = crate::assets::search_assets_for_agent(
                &state.connection,
                state.project_id,
                args.get("query").and_then(Value::as_str),
                args.get("kind").and_then(Value::as_str),
                args.get("minDurationMs").and_then(Value::as_i64),
                args.get("maxDurationMs").and_then(Value::as_i64),
                args.get("minRating").and_then(Value::as_i64),
                args.get("favoriteOnly")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                args.get("tag").and_then(Value::as_str),
                args.get("collectionId").and_then(Value::as_str),
                args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize,
                args.get("limit").and_then(Value::as_u64).unwrap_or(12) as usize,
            )?;
            Ok(json!({ "tool": "search_assets", "status": "ok", "results": results }))
        }
        "search_asset_segments" => {
            let query = args
                .get("query")
                .and_then(Value::as_str)
                .ok_or_else(|| "Segments query required.".to_owned())?;
            let results = crate::assets::search_asset_segments_for_agent(
                &state.connection,
                state.project_id,
                query,
                args.get("assetId").and_then(Value::as_str),
                args.get("offset").and_then(Value::as_u64).unwrap_or(0) as usize,
                args.get("limit").and_then(Value::as_u64).unwrap_or(12) as usize,
            )?;
            Ok(json!({"tool":"search_asset_segments","status":"ok","results":results}))
        }
        "request_asset_analysis" => {
            let asset_ids: Vec<String> = serde_json::from_value(
                args.get("assetIds")
                    .cloned()
                    .ok_or_else(|| "request_asset_analysis needs an assetIds array.".to_owned())?,
            )
            .map_err(|error| error.to_string())?;
            let queued =
                crate::assets::request_asset_analysis(state.app, state.project_id, &asset_ids)?;
            Ok(
                json!({ "tool": "request_asset_analysis", "status": "queued", "queuedCount": queued }),
            )
        }
        "get_storyboard" => Ok(json!({
            "tool": "get_storyboard",
            "status": "ok",
            "storyboard": state
                .storyboard
                .as_ref()
                .map(|value| serde_json::to_value(value).unwrap_or_else(|e| {
                    log::warn!("Storyboard could not be serialized for model context: {e}");
                    Value::Null
                }))
                .unwrap_or(Value::Null)
        })),
        "get_timeline" => {
            let timeline_id = args.get("timelineVersionId").and_then(Value::as_str);
            Ok(json!({
                "tool": "get_timeline",
                "status": "ok",
                "timeline": build_timeline_snapshot(state, timeline_id)
            }))
        }
        "get_text_capabilities" => Ok(json!({
            "tool": "get_text_capabilities",
            "status": "ok",
            "fonts": [
                {"fontKey": "jianying_default", "preview": "supported", "jianying": "verified", "note": "Jianying default font; Unicode text is written through the verified escaped nested-text path."},
                {"fontKey": "sans_bold", "preview": "supported", "jianying": "local_preview_only"},
                {"fontKey": "sans_clean", "preview": "supported", "jianying": "local_preview_only"},
                {"fontKey": "serif_editorial", "preview": "supported", "jianying": "local_preview_only"},
                {"fontKey": "mono_tech", "preview": "supported", "jianying": "local_preview_only"},
                {"fontKey": "jianying_sans_bold", "preview": "supported", "jianying": "local_preview_only", "note": "Writes the Jianying Source Han Sans bold resource; visual delivery validation is pending."},
                {"fontKey": "jianying_sans_regular", "preview": "supported", "jianying": "local_preview_only", "note": "Writes the Jianying Source Han Sans regular resource; visual delivery validation is pending."},
                {"fontKey": "jianying_serif_bold", "preview": "supported", "jianying": "local_preview_only", "note": "Writes the Jianying Source Han Serif bold resource; visual delivery validation is pending."},
                {"fontKey": "jianying_handwritten", "preview": "supported", "jianying": "local_preview_only", "note": "Writes the Jianying WenKai bold resource; visual delivery validation is pending."},
                {"fontKey": "jianying_harmony_bold", "preview": "supported", "jianying": "local_preview_only", "note": "Writes the Jianying HarmonyOS Sans bold resource; visual delivery validation is pending."}
            ],
            "templates": [
                {"templateId": "static", "preview": "supported", "jianying": "verified"},
                {"templateId": "fade", "phase": "entrance_or_exit", "preview": "supported", "jianying": "verified"},
                {"templateId": "slide_up", "phase": "entrance", "preview": "supported", "jianying": "verified"},
                {"templateId": "slide_down", "phase": "entrance", "preview": "supported", "jianying": "verified"},
                {"templateId": "pop", "phase": "entrance", "preview": "supported", "jianying": "verified"},
                {"templateId": "wipe", "preview": "supported", "jianying": "local_preview_only"}
            ],
            "textRecipes": text_recipe_capabilities(),
            "jianyingRestrictions": "Verified delivery requires jianying_default, no stroke, shadow, background, or loop animation; only fade may be an exit, and only fade/slide_up/slide_down/pop may be an entrance. Text content is serialized through the verified escaped nested-text path."
        })),
        "generate_storyboard" => {
            let brief = args
                .get("brief")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|brief| !brief.is_empty())
                .unwrap_or(state.task_brief.trim());
            if brief.is_empty() {
                return Err("The user has no video goal to base a storyboard on.".to_owned());
            }
            let generated = crate::storyboard::generate_storyboard_for_agent(
                state.app.clone(),
                state.project_id.to_owned(),
                state.editing_task_id.to_owned(),
                brief.to_owned(),
            )?;
            let storyboard_version_id = generated.id.clone();
            let version_number = generated.version_number;
            let summary = generated.summary.clone();
            state.storyboard = Some(generated.clone());
            state.timelines = Vec::new();
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: format!(
                    "已按你的目标生成 storyboard（版本 {version}）。{summary}\n\n请确认该 storyboard，确认后系统将自动创建时间线并生成预览。",
                    version = version_number
                ),
                storyboard: Some(generated),
                timeline: None,
                preview: None,
                jianying_draft: None,
            });
            Ok(json!({
                "tool": "generate_storyboard",
                "status": "needs_confirmation",
                "storyboardVersionId": storyboard_version_id,
                "versionNumber": version_number
            }))
        }
        "create_timeline_draft" => {
            let storyboard = state
                .storyboard
                .as_ref()
                .ok_or_else(|| "Create a storyboard before creating a timeline.".to_owned())?;
            let created = create_timeline_draft(
                state.app.clone(),
                state.project_id.to_owned(),
                storyboard.id.clone(),
            )?;
            let timeline_version_id = created.id.clone();
            let version_number = created.version_number;
            upsert(&mut state.timelines, created.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: format!("已创建内容时间线 v{}。", version_number),
                storyboard: None,
                timeline: Some(created),
                preview: None,
                jianying_draft: None,
            });
            Ok(json!({
                "tool": "create_timeline_draft",
                "status": "ok",
                "timelineVersionId": timeline_version_id,
                "versionNumber": version_number
            }))
        }
        "replace_clips" => {
            let existing = select_timeline_for_tool(state, args)?;
            let shots_json = args
                .get("shots")
                .or_else(|| args.get("replacements"))
                .ok_or_else(|| "replace_clips needs a shots array.".to_owned())?;
            let params: Vec<ClipReplacementParams> =
                serde_json::from_value(shots_json.clone()).map_err(|error| error.to_string())?;
            if params.is_empty() {
                return Err("Agent did not identify any replacement media.".to_owned());
            }
            let replacements: Vec<ClipReplacement> = params
                .into_iter()
                .map(|replacement| ClipReplacement {
                    shot_index: replacement.shot_index,
                    asset_id: replacement.asset_id,
                    source_start_ms: replacement.source_start_ms,
                    source_end_ms: replacement.source_end_ms,
                })
                .collect();
            let result = replace_clips(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                state.agent_task_id,
                &existing,
                &replacements,
            )?;
            let timeline_version_id = result.id.clone();
            let version_number = result.version_number;
            let quality_warnings = text_track_quality_warnings(&result.text_tracks);
            upsert(&mut state.timelines, result.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: format!("已批量替换镜头并创建新的内部时间线 v{}。", version_number),
                storyboard: None,
                timeline: Some(result),
                preview: None,
                jianying_draft: None,
            });
            Ok(json!({
                "tool": "replace_clips",
                "status": "ok",
                "timelineVersionId": timeline_version_id,
                "versionNumber": version_number,
                "qualityWarnings": quality_warnings
            }))
        }
        "change_clip_duration" => {
            let existing = select_timeline_for_tool(state, args)?;
            let adjustments_json = args
                .get("adjustments")
                .ok_or_else(|| "change_clip_duration needs an adjustments array.".to_owned())?;
            let adjustments: Vec<ClipAdjustmentParams> =
                serde_json::from_value(adjustments_json.clone())
                    .map_err(|error| error.to_string())?;
            if adjustments.is_empty() {
                return Err("Agent did not identify any clips to retime.".to_owned());
            }
            let clip_adjustments: Vec<ClipAdjustment> = adjustments
                .into_iter()
                .map(|adjustment| ClipAdjustment {
                    shot_index: adjustment.shot_index,
                    new_duration_ms: adjustment.new_duration_ms,
                    new_source_start_ms: adjustment.new_source_start_ms,
                })
                .collect();
            let result = change_clip_duration(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                state.agent_task_id,
                &existing,
                &clip_adjustments,
            )?;
            let timeline_version_id = result.id.clone();
            let version_number = result.version_number;
            upsert(&mut state.timelines, result.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: format!(
                    "已按新的时长与起止点校准镜头并创建本地时间线 v{}。",
                    version_number
                ),
                storyboard: None,
                timeline: Some(result),
                preview: None,
                jianying_draft: None,
            });
            Ok(json!({
                "tool": "change_clip_duration",
                "status": "ok",
                "timelineVersionId": timeline_version_id,
                "versionNumber": version_number
            }))
        }
        "reorder_clips" => {
            let existing = select_timeline_for_tool(state, args)?;
            let order_json = args
                .get("order")
                .ok_or_else(|| "reorder_clips needs an order array.".to_owned())?;
            let order: Vec<i64> =
                serde_json::from_value(order_json.clone()).map_err(|error| error.to_string())?;
            let result = reorder_clips(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                state.agent_task_id,
                &existing,
                &order,
            )?;
            let timeline_version_id = result.id.clone();
            let version_number = result.version_number;
            upsert(&mut state.timelines, result.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: format!("已按新的顺序排列镜头并创建本地时间线 v{}。", version_number),
                storyboard: None,
                timeline: Some(result),
                preview: None,
                jianying_draft: None,
            });
            Ok(json!({
                "tool": "reorder_clips",
                "status": "ok",
                "timelineVersionId": timeline_version_id
            }))
        }
        "replace_text_tracks" => {
            let existing = select_timeline_for_tool(state, args)?;
            let tracks_json = args
                .get("textTracks")
                .ok_or_else(|| "replace_text_tracks needs a textTracks array.".to_owned())?;
            let text_tracks: Vec<TextTrack> =
                serde_json::from_value(tracks_json.clone()).map_err(|error| error.to_string())?;
            let result = replace_text_tracks(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                state.agent_task_id,
                &existing,
                text_tracks,
            )?;
            let timeline_version_id = result.id.clone();
            let version_number = result.version_number;
            upsert(&mut state.timelines, result.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: format!("已更新文本轨并创建内部时间线 v{}。", version_number),
                storyboard: None,
                timeline: Some(result),
                preview: None,
                jianying_draft: None,
            });
            Ok(json!({
                "tool": "replace_text_tracks",
                "status": "ok",
                "timelineVersionId": timeline_version_id,
                "versionNumber": version_number
            }))
        }
        "replace_music_tracks" => {
            let existing = select_timeline_for_tool(state, args)?;
            let tracks_json = args
                .get("musicTracks")
                .ok_or_else(|| "replace_music_tracks needs a musicTracks array.".to_owned())?;
            let music_tracks: Vec<MusicTrack> =
                serde_json::from_value(tracks_json.clone()).map_err(|error| error.to_string())?;
            let result = replace_music_tracks(
                state.connection,
                state.project_id,
                state.editing_task_id,
                state.conversation_id,
                &agent_task_id,
                &existing,
                music_tracks,
            )?;
            let timeline_version_id = result.id.clone();
            let version_number = result.version_number;
            upsert(&mut state.timelines, result.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: "已创建本地音乐轨时间线版本；可创建新的实验性 Jianying music draft，仍需在 Jianying 中复核播放效果。".to_owned(),
                storyboard: None,
                timeline: Some(result),
                preview: None,
                jianying_draft: None,
            });
            Ok(
                json!({"tool":"replace_music_tracks","status":"ok","timelineVersionId":timeline_version_id,"versionNumber":version_number,"jianying":"experimental_review_required"}),
            )
        }
        "render_preview" => {
            let timeline = select_timeline_for_tool(state, args)?;
            let timeline_version_id = timeline.id.clone();
            let version_number = timeline.version_number;
            let timeline_for_render = timeline.clone();
            let preview = render_preview(state.app.clone(), timeline_for_render.id.clone())?;
            let quality_check_count = preview.quality_report.checks.len();
            upsert(&mut state.timelines, timeline_for_render.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message: format!("已按请求生成本地低清预览（时间线 v{}）。", version_number),
                storyboard: None,
                timeline: Some(timeline_for_render),
                preview: Some(preview),
                jianying_draft: None,
            });
            Ok(json!({
                "tool": "render_preview",
                "status": "ok",
                "artifact": {
                    "type": "preview",
                    "timelineVersionId": timeline_version_id,
                    "versionNumber": version_number,
                    "qualityCheckCount": quality_check_count
                }
            }))
        }
        "create_jianying_draft" => {
            let timeline = select_timeline_for_tool(state, args)?;
            let timeline_version_id = timeline.id.clone();
            let draft_timeline = timeline.clone();
            let draft = create_jianying_draft(state.app.clone(), draft_timeline.id.clone())?;
            let draft_name = Path::new(&draft.draft_directory)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("Assembly Video Agent");
            let message = if draft.registration_status == "pending" {
                format!("已生成剪映草稿\u{201c}{draft_name}\u{201d}，剪映正在运行，退出剪映后会自动完成注册。")
            } else {
                format!(
                    "已创建并注册剪映草稿\u{201c}{draft_name}\u{201d}，可在剪映本地草稿中查看。"
                )
            };
            let registration_status = draft.registration_status.clone();
            upsert_timeline(&mut state.timelines, draft_timeline.clone());
            state.last_outcome = Some(AgentEditResult {
                agent_task_id,
                message,
                storyboard: None,
                timeline: Some(draft_timeline),
                preview: None,
                jianying_draft: Some(draft),
            });
            Ok(json!({
                "tool": "create_jianying_draft",
                "status": "ok",
                "timelineVersionId": timeline_version_id,
                "registrationStatus": registration_status
            }))
        }
        other => Err(format!("Unknown skill: {other}")),
    }
}
