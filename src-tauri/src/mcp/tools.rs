//! MCP 工具：列项目、检索素材、取片段证据、按镜头清单新建时间线并交付。
//! 检索与证据走现有 library / evidence_contract；交付走现有 timeline 与 handoff，不另写一套。

use crate::db::{open_connection, now_millis};
use crate::handoff::deliver::{deliver_to_editor, EditorDeliveryResult};
use crate::handoff::EditorId;
use crate::media_options::{AspectRatio, MediaOptions};
use crate::models::{
    AssetEvidence, SceneSegment, StoryboardBeat, StoryboardContent, StoryboardDerivation,
    StoryboardShot, TechnicalMetadata,
};
use crate::storyboard::insert_storyboard_version;
use crate::timeline::create_timeline_draft_with_options;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use tauri::AppHandle;

const MAX_SHOTS: usize = 100;
const MCP_SESSION_TITLE: &str = "MCP";

pub(crate) fn catalog() -> Vec<Value> {
    vec![
        tool(
            "list_projects",
            "List Voycut projects in the local library. Returns id, name, and timestamps. Does not expose credentials.",
            json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        ),
        tool(
            "list_assets",
            "List assets available to a project (display name, kind, duration, analysis status, scene count). Does not run analysis or return credentials.",
            json!({
                "type": "object",
                "properties": {
                    "projectId": { "type": "string", "minLength": 1, "description": "Project id from list_projects." }
                },
                "required": ["projectId"],
                "additionalProperties": false
            }),
        ),
        tool(
            "search_assets",
            "Search a project's analyzed footage by text. Returns matching assets with segments, source time ranges, visual captions, and risk states (hit / not_hit / unknown). Uses persisted evidence only; does not spend model quota.",
            json!({
                "type": "object",
                "properties": {
                    "projectId": { "type": "string", "minLength": 1 },
                    "query": { "type": ["string", "null"], "minLength": 1, "maxLength": 200, "description": "Text query over names, tags, OCR, and visual captions. Null lists a page." },
                    "kind": { "type": ["string", "null"], "enum": ["video", "image", "audio", "other"] },
                    "offset": { "type": "integer", "minimum": 0, "maximum": 10000, "default": 0 },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 20, "default": 20 }
                },
                "required": ["projectId"],
                "additionalProperties": false
            }),
        ),
        tool(
            "get_asset_evidence",
            "Return one asset's persisted segment evidence, risk triad, visual descriptions, and keyframe image paths. Does not run analysis.",
            json!({
                "type": "object",
                "properties": {
                    "projectId": { "type": "string", "minLength": 1 },
                    "assetId": { "type": "string", "minLength": 1, "maxLength": 200 }
                },
                "required": ["projectId", "assetId"],
                "additionalProperties": false
            }),
        ),
        tool(
            "create_and_deliver_timeline",
            "Create a new internal timeline from an ordered shot list (asset id + source in/out) and deliver a new editor draft. Editors: jianying, capcut, fcpxml, otio. Never overwrites an existing draft. Invalid assets, empty lists, or out-of-range times return the real error. Does not run analysis or spend model quota.",
            json!({
                "type": "object",
                "properties": {
                    "projectId": { "type": "string", "minLength": 1 },
                    "editorId": { "type": "string", "enum": ["jianying", "capcut", "fcpxml", "otio"] },
                    "title": { "type": ["string", "null"], "maxLength": 120 },
                    "aspectRatio": { "type": ["string", "null"], "enum": ["9:16", "16:9", "1:1"] },
                    "shots": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 100,
                        "items": {
                            "type": "object",
                            "properties": {
                                "assetId": { "type": "string", "minLength": 1 },
                                "sourceStartMs": { "type": "integer", "minimum": 0 },
                                "sourceEndMs": { "type": "integer", "minimum": 0 },
                                "segmentId": { "type": ["string", "null"] },
                                "subtitleText": { "type": ["string", "null"], "maxLength": 280 },
                                "audioText": { "type": ["string", "null"], "maxLength": 2000, "description": "Optional narration or audio note stored on the shot; not synthesized." }
                            },
                            "required": ["assetId", "sourceStartMs", "sourceEndMs"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["projectId", "editorId", "shots"],
                "additionalProperties": false
            }),
        ),
    ]
}

pub(crate) fn dispatch(app: &AppHandle, name: &str, arguments: Value) -> Result<Value, String> {
    match name {
        "list_projects" => list_projects(app),
        "list_assets" => list_assets(app, arguments),
        "search_assets" => search_assets(app, arguments),
        "get_asset_evidence" => get_asset_evidence(app, arguments),
        "create_and_deliver_timeline" => create_and_deliver_timeline(app, arguments),
        _ => Err(format!("Unknown tool: {name}")),
    }
}

fn list_projects(app: &AppHandle) -> Result<Value, String> {
    let projects = crate::projects::list_projects(app.clone())?;
    Ok(json!({ "projects": projects }))
}

fn list_assets(app: &AppHandle, arguments: Value) -> Result<Value, String> {
    let project_id = required_string(&arguments, "projectId")?;
    ensure_project(app, &project_id)?;
    let assets = crate::assets::list_assets_for_agent(app.clone(), project_id)?;
    Ok(json!({ "assets": assets, "count": assets.len() }))
}

fn search_assets(app: &AppHandle, arguments: Value) -> Result<Value, String> {
    let project_id = required_string(&arguments, "projectId")?;
    ensure_project(app, &project_id)?;
    let query = optional_string(&arguments, "query");
    let kind = optional_string(&arguments, "kind");
    let offset = arguments
        .get("offset")
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .unwrap_or(20) as usize;
    let connection = open_connection(app)?;
    let mut page = crate::assets::search_assets_for_agent(
        &connection,
        &project_id,
        query.as_deref(),
        kind.as_deref(),
        None,
        None,
        None,
        false,
        None,
        None,
        offset,
        limit,
    )?;
    let Some(candidates) = page.get_mut("candidates").and_then(Value::as_array_mut) else {
        return Ok(page);
    };
    for candidate in candidates {
        let Some(asset_id) = candidate.get("assetId").and_then(Value::as_str) else {
            continue;
        };
        let Ok(evidence) = crate::assets::load_asset_evidence(&connection, asset_id) else {
            continue;
        };
        candidate["segments"] = json!(compact_segments(&evidence));
    }
    Ok(page)
}

fn get_asset_evidence(app: &AppHandle, arguments: Value) -> Result<Value, String> {
    let project_id = required_string(&arguments, "projectId")?;
    let asset_id = required_string(&arguments, "assetId")?;
    ensure_project(app, &project_id)?;
    let connection = open_connection(app)?;
    ensure_project_asset(&connection, &project_id, &asset_id)?;
    let evidence = crate::assets::load_asset_evidence(&connection, &asset_id)?;
    serde_json::to_value(evidence).map_err(|error| error.to_string())
}

fn create_and_deliver_timeline(app: &AppHandle, arguments: Value) -> Result<Value, String> {
    let request: DeliverRequest =
        serde_json::from_value(arguments).map_err(|error| error.to_string())?;
    if request.shots.is_empty() {
        return Err("Shot list is empty.".to_owned());
    }
    if request.shots.len() > MAX_SHOTS {
        return Err(format!("Shot list cannot exceed {MAX_SHOTS} shots."));
    }
    let editor = EditorId::parse(&request.editor_id)?;
    ensure_project(app, &request.project_id)?;
    let connection = open_connection(app)?;
    let shots = resolve_shots(&connection, &request.project_id, &request.shots)?;
    drop(connection);
    let editing_task_id = ensure_mcp_editing_task(app, &request.project_id)?;
    let title = request
        .title
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("MCP timeline")
        .to_owned();
    let has_subtitles = shots
        .iter()
        .any(|shot| !shot.on_screen_text.trim().is_empty());
    let media_options = MediaOptions {
        voiceover: false,
        subtitles: has_subtitles,
        bgm: false,
        aspect_ratio: parse_aspect_ratio(request.aspect_ratio.as_deref())?,
        genre: Default::default(),
    };
    let content = storyboard_from_shots(&title, &shots);
    let connection = open_connection(app)?;
    let storyboard = insert_storyboard_version(
        &connection,
        request.project_id.clone(),
        &editing_task_id,
        &title,
        content,
        &[],
        Some(media_options),
        StoryboardDerivation::default(),
    )?;
    drop(connection);
    let timeline = create_timeline_draft_with_options(
        app.clone(),
        request.project_id.clone(),
        storyboard.id.clone(),
        Some(media_options),
    )?;
    let delivery: EditorDeliveryResult =
        deliver_to_editor(app.clone(), timeline.id.clone(), Some(editor.as_str().to_owned()))?;
    Ok(json!({
        "storyboardVersionId": storyboard.id,
        "timelineVersionId": timeline.id,
        "editorId": delivery.editor_id,
        "status": delivery.status,
        "outputPath": delivery.output_path,
        "displayName": delivery.display_name,
        "message": delivery.message,
        "notes": delivery.notes,
        "createdAt": now_millis(),
    }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeliverRequest {
    project_id: String,
    editor_id: String,
    title: Option<String>,
    aspect_ratio: Option<String>,
    shots: Vec<ShotInput>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShotInput {
    asset_id: String,
    source_start_ms: i64,
    source_end_ms: i64,
    segment_id: Option<String>,
    subtitle_text: Option<String>,
    audio_text: Option<String>,
}

#[derive(Debug, Clone)]
struct ResolvedAsset {
    kind: String,
    duration_ms: Option<i64>,
    source_reference: String,
    segments: Vec<SceneSegment>,
}

fn validate_shot_against_asset(
    index: usize,
    shot: &ShotInput,
    asset: &ResolvedAsset,
) -> Result<(), String> {
    let n = index + 1;
    if shot.source_end_ms <= shot.source_start_ms {
        return Err(format!(
            "Shot {n} source range {}-{} ms is empty or reversed.",
            shot.source_start_ms, shot.source_end_ms
        ));
    }
    if shot.source_start_ms < 0 {
        return Err(format!("Shot {n} sourceStartMs cannot be negative."));
    }
    if !Path::new(&asset.source_reference).is_file() {
        return Err(format!(
            "Source media for asset {} is unavailable.",
            shot.asset_id
        ));
    }
    match asset.kind.as_str() {
        "video" => {
            let duration = asset.duration_ms.ok_or_else(|| {
                format!(
                    "Asset {} has no verified duration; finish analysis before using it in a timeline.",
                    shot.asset_id
                )
            })?;
            if shot.source_end_ms > duration {
                return Err(format!(
                    "Shot {n} source range {}-{} ms is outside asset {} duration {} ms.",
                    shot.source_start_ms, shot.source_end_ms, shot.asset_id, duration
                ));
            }
        }
        "image" => {
            if shot.source_start_ms != 0 || shot.source_end_ms != 0 {
                return Err(format!(
                    "Shot {n} image asset {} must use source range 0-0 ms.",
                    shot.asset_id
                ));
            }
        }
        kind => {
            return Err(format!(
                "Shot {n} asset {} has kind {kind}, which cannot be used as a picture shot.",
                shot.asset_id
            ));
        }
    }
    if let Some(segment_id) = shot
        .segment_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        let segment = asset
            .segments
            .iter()
            .find(|segment| segment.id == segment_id)
            .ok_or_else(|| {
                format!(
                    "Shot {n} segment {segment_id} was not found on asset {}.",
                    shot.asset_id
                )
            })?;
        if shot.source_start_ms < segment.start_ms || shot.source_end_ms > segment.end_ms {
            return Err(format!(
                "Shot {n} source range {}-{} ms is outside segment {segment_id} {}-{} ms.",
                shot.source_start_ms, shot.source_end_ms, segment.start_ms, segment.end_ms
            ));
        }
    }
    Ok(())
}

fn resolve_shots(
    connection: &Connection,
    project_id: &str,
    shots: &[ShotInput],
) -> Result<Vec<StoryboardShot>, String> {
    let mut resolved = Vec::with_capacity(shots.len());
    for (index, shot) in shots.iter().enumerate() {
        let asset = load_shot_asset(connection, project_id, &shot.asset_id)?;
        validate_shot_against_asset(index, shot, &asset)?;
        let duration_ms = if asset.kind == "image" {
            1_000
        } else {
            shot.source_end_ms - shot.source_start_ms
        };
        let subtitle = shot
            .subtitle_text
            .as_deref()
            .unwrap_or("")
            .trim()
            .chars()
            .take(280)
            .collect::<String>();
        let narration = shot
            .audio_text
            .as_deref()
            .unwrap_or("")
            .trim()
            .chars()
            .take(2000)
            .collect::<String>();
        let segment_id = shot
            .segment_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        resolved.push(StoryboardShot {
            crop_focus: None,
            order_index: (index as i64) + 1,
            duration_ms,
            purpose: format!("MCP shot {}", index + 1),
            on_screen_text: subtitle,
            narration_text: narration,
            asset_id: shot.asset_id.clone(),
            source_start_ms: shot.source_start_ms,
            source_end_ms: shot.source_end_ms,
            reason: "External Agent shot list".to_owned(),
            beat_id: format!("mcp-{:03}", index + 1),
            match_level: "direct".to_owned(),
            beat_part_index: 1,
            beat_part_count: 1,
            split_role: "lead".to_owned(),
            segment_id,
        });
    }
    Ok(resolved)
}

fn load_shot_asset(
    connection: &Connection,
    project_id: &str,
    asset_id: &str,
) -> Result<ResolvedAsset, String> {
    let row = connection
        .query_row(
            "SELECT kind, metadata_json, source_reference FROM assets
             WHERE id = ?1
               AND id IN (SELECT asset_id FROM project_asset_access WHERE project_id = ?2)
               AND coalesce(json_extract(metadata_json, '$.libraryRemoved'), 0) = 0
               AND coalesce((SELECT excluded FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0) = 0",
            params![asset_id, project_id],
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
    let Some((kind, metadata_json, source_reference)) = row else {
        return Err(format!("Asset {asset_id} was not found in this project."));
    };
    let metadata: TechnicalMetadata = serde_json::from_str(&metadata_json).unwrap_or_default();
    Ok(ResolvedAsset {
        kind,
        duration_ms: metadata.duration_ms,
        source_reference,
        segments: metadata.scene_segments,
    })
}

fn storyboard_from_shots(title: &str, shots: &[StoryboardShot]) -> StoryboardContent {
    let target_duration_ms = shots.iter().map(|shot| shot.duration_ms).sum::<i64>().max(1);
    let beats = shots
        .iter()
        .map(|shot| StoryboardBeat {
            id: shot.beat_id.clone(),
            purpose: shot.purpose.clone(),
            required_visual: shot.reason.clone(),
            visual_keywords: Vec::new(),
            narration: shot.narration_text.clone(),
            on_screen_text: shot.on_screen_text.clone(),
            pace_hint: String::new(),
        })
        .collect();
    StoryboardContent {
        brief: title.to_owned(),
        title: title.to_owned(),
        summary: "Timeline created from an external Agent shot list.".to_owned(),
        target_duration_ms,
        script_mode: "key_message".to_owned(),
        beats,
        uncovered_beat_ids: Vec::new(),
        shots: shots.to_vec(),
    }
}

fn ensure_mcp_editing_task(app: &AppHandle, project_id: &str) -> Result<String, String> {
    let connection = open_connection(app)?;
    let existing: Option<String> = connection
        .query_row(
            "SELECT id FROM editing_tasks WHERE project_id = ?1 AND title = ?2 ORDER BY created_at ASC LIMIT 1",
            params![project_id, MCP_SESSION_TITLE],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if let Some(id) = existing {
        return Ok(id);
    }
    drop(connection);
    let session =
        crate::projects::create_editing_session(app.clone(), project_id.to_owned(), MCP_SESSION_TITLE.to_owned())?;
    Ok(session.id)
}

fn ensure_project(app: &AppHandle, project_id: &str) -> Result<(), String> {
    let connection = open_connection(app)?;
    let exists: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM projects WHERE id = ?1",
            params![project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if exists == 0 {
        return Err(format!("Project {project_id} was not found."));
    }
    Ok(())
}

fn ensure_project_asset(
    connection: &Connection,
    project_id: &str,
    asset_id: &str,
) -> Result<(), String> {
    let exists: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM assets
             WHERE id = ?1
               AND id IN (SELECT asset_id FROM project_asset_access WHERE project_id = ?2)
               AND coalesce(json_extract(metadata_json, '$.libraryRemoved'), 0) = 0",
            params![asset_id, project_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if exists == 0 {
        return Err(format!("Asset {asset_id} was not found in this project."));
    }
    Ok(())
}

fn compact_segments(evidence: &AssetEvidence) -> Value {
    json!(evidence
        .segment_evidence
        .iter()
        .map(|segment| {
            json!({
                "segmentId": segment.segment_id,
                "evidenceId": segment.id,
                "sourceStartMs": segment.range.start_ms,
                "sourceEndMs": segment.range.end_ms,
                "caption": segment.caption,
                "narrativeRole": segment.narrative_role,
                "risks": segment.risks.iter().map(|risk| json!({
                    "risk": risk.risk,
                    "state": risk.state,
                    "startMs": risk.range.start_ms,
                    "endMs": risk.range.end_ms,
                    "confidence": risk.confidence,
                    "method": risk.source.method,
                    "value": risk.value
                })).collect::<Vec<_>>(),
            })
        })
        .collect::<Vec<_>>())
}

fn parse_aspect_ratio(raw: Option<&str>) -> Result<AspectRatio, String> {
    match raw.map(str::trim).filter(|value| !value.is_empty()) {
        None => Ok(AspectRatio::default()),
        Some("9:16") => Ok(AspectRatio::Portrait),
        Some("16:9") => Ok(AspectRatio::Landscape),
        Some("1:1") => Ok(AspectRatio::Square),
        Some(other) => Err(format!("Unsupported aspectRatio {other}. Use 9:16, 16:9, or 1:1.")),
    }
}

fn required_string(value: &Value, key: &str) -> Result<String, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("{key} is required."))
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}

fn tool(name: &str, description: &str, input_schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": input_schema
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video_asset(duration_ms: i64) -> ResolvedAsset {
        ResolvedAsset {
            kind: "video".to_owned(),
            duration_ms: Some(duration_ms),
            source_reference: file!().to_owned(),
            segments: vec![SceneSegment {
                id: "s001".to_owned(),
                start_ms: 0,
                end_ms: 4_000,
                scene_duration_ms: None,
                visual_quality_score: None,
                frames: Vec::new(),
                visual_evidence: None,
                motion_score: None,
                motion_profile: None,
            }],
        }
    }

    fn shot(start: i64, end: i64) -> ShotInput {
        ShotInput {
            asset_id: "asset-1".to_owned(),
            source_start_ms: start,
            source_end_ms: end,
            segment_id: None,
            subtitle_text: None,
            audio_text: None,
        }
    }

    #[test]
    fn rejects_empty_or_reversed_range() {
        let asset = video_asset(10_000);
        let error = validate_shot_against_asset(0, &shot(1_000, 1_000), &asset).unwrap_err();
        assert!(error.contains("empty or reversed"), "{error}");
    }

    #[test]
    fn rejects_range_past_duration() {
        let asset = video_asset(5_000);
        let error = validate_shot_against_asset(0, &shot(0, 6_000), &asset).unwrap_err();
        assert!(error.contains("outside asset"), "{error}");
    }

    #[test]
    fn rejects_unknown_segment() {
        let asset = video_asset(10_000);
        let mut shot = shot(0, 1_000);
        shot.segment_id = Some("s999".to_owned());
        let error = validate_shot_against_asset(0, &shot, &asset).unwrap_err();
        assert!(error.contains("was not found"), "{error}");
    }

    #[test]
    fn accepts_in_range_video() {
        let asset = video_asset(10_000);
        validate_shot_against_asset(0, &shot(1_000, 2_500), &asset).unwrap();
    }
}
