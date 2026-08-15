//! 只使用已就绪真实媒体证据生成版本化 storyboard，并校验证据源时间范围。
//! 文件名和路径只能用于本地组织，不能冒充媒体内容证据。

use crate::assets::{prioritize_pending_visual_batches, wait_for_visual_batch};
use crate::db::{now_millis, open_connection};
use crate::models::{
    StoryboardBeat, StoryboardContent, StoryboardShot, StoryboardSource, StoryboardVersion,
    TechnicalMetadata,
};
use crate::provider::{model_response_json_text, post_model_payload, ModelAccess};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::time::Duration;
use tauri::AppHandle;
use uuid::Uuid;

/// Timeout for a single storyboard generation model request so a slow or hung
/// provider never blocks the agent loop forever.
const STORYBOARD_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_STORYBOARD_REVISIONS: usize = 3;

fn storyboard_repair_message(message: impl Into<String>, shot_indices: Vec<i64>) -> String {
    let message = message.into();
    if shot_indices.is_empty() {
        message
    } else {
        format!(
            "{message} Affected shot indices: {}.",
            shot_indices
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    }
}

pub(crate) fn storyboard_sources(
    connection: &Connection,
    project_id: &str,
) -> Result<(Vec<StoryboardSource>, usize), String> {
    let mut statement = connection.prepare(
        "SELECT id, kind, metadata_json, source_reference FROM assets WHERE project_id = ?1 AND analysis_status = 'ready' AND coalesce((SELECT excluded FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0) = 0",
    ).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            let metadata: TechnicalMetadata =
                serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or_default();
            let visual_ready =
                metadata.visual_analysis_status == "ready" && !metadata.visual_evidence.is_empty();
            let source_available = Path::new(&row.get::<_, String>(3)?).is_file();
            Ok((
                StoryboardSource {
                    asset_id: row.get(0)?,
                    kind: row.get(1)?,
                    duration_ms: metadata.duration_ms,
                    scene_segments: metadata.scene_segments,
                    ocr_evidence: metadata.ocr_evidence,
                    visual_evidence: metadata.visual_evidence,
                },
                visual_ready,
                visual_ready && source_available,
            ))
        })
        .map_err(|error| error.to_string())?;
    let candidates = rows
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    let visual_ready_count = candidates
        .iter()
        .filter(|(_, visual_ready, _)| *visual_ready)
        .count();
    Ok((
        candidates
            .into_iter()
            .filter_map(|(source, _, available)| available.then_some(source))
            .collect(),
        visual_ready_count,
    ))
}

pub(crate) fn request_storyboard(
    access: &ModelAccess,
    brief: &str,
    sources: &[StoryboardSource],
    previous: Option<&StoryboardContent>,
    feedback: Option<&str>,
) -> Result<StoryboardContent, String> {
    let evidence = serde_json::to_string(sources)
        .map_err(|_| "Could not prepare media evidence.".to_owned())?;
    let previous_json = previous
        .map(serde_json::to_string)
        .transpose()
        .map_err(|_| "Could not prepare storyboard revision context.".to_owned())?
        .unwrap_or_default();
    let revision_context = feedback.map_or(String::new(), |feedback| {
        format!("\nPrevious attempt failed local validation: {feedback}\nPrevious storyboard JSON (revise it when useful): {previous_json}\n")
    });
    let prompt = format!(
        "Create an editable, evidence-bound storyboard for this brief: {brief}\n\
        Work in two stages inside the returned JSON: first split the brief into the narrative beats it needs (up to 30 for safety); then choose the strongest real media for each covered beat.\n\
        Return title, summary, targetDurationMs, scriptMode, beats, uncoveredBeatIds, and shots. targetDurationMs is your creative duration proposal (3-120 seconds). scriptMode must be full_script or key_message. Each beat must contain id, purpose, and requiredVisual.\n\
        Each shot must contain orderIndex, durationMs, purpose, onScreenText, assetId, sourceStartMs, sourceEndMs, reason, beatId, and matchLevel.\n\
        matchLevel must be direct or contextual. Use direct only when the supplied evidence visibly and specifically supports the beat. Use contextual only for honest scene-setting footage, and say exactly what is contextual in reason.\n\
        Prefer the clearest, most specific, and least repetitive evidence. Do not default to the first source, the longest source, or a generic clip if a more relevant one exists. Avoid padding a beat with weak footage when a better shot is available.\n\
        Never output an insufficient shot. If no supplied media can honestly support a beat, put its id in uncoveredBeatIds and do not create a standalone shot for it.\n\
        Every beat must be covered by at least one shot or appear exactly once in uncoveredBeatIds. Avoid overlapping or repeated source ranges from the same asset unless the brief explicitly requires a repeat.\n\
        Use ONLY the supplied media evidence JSON below. For video, source times must be inside the provided duration and preferably align with sceneSegments. For images, sourceStartMs and sourceEndMs must both be 0. Do not use file names, unknown asset IDs, or unverified claims.\n\
        Evidence: {evidence}{revision_context}"
    );
    let model_name = access
        .custom_config()
        .map(|config| {
            if config.coarse_visual_model.is_empty() {
                config.model.as_str()
            } else {
                config.coarse_visual_model.as_str()
            }
        })
        .unwrap_or("gpt-5.4");
    let request = serde_json::json!({
        "model": model_name,
        "store": false,
        "stream": true,
        "input": [{ "role": "user", "content": [{ "type": "input_text", "text": prompt }] }],
        "text": { "format": { "type": "json_object" } }
    });
    let body = post_model_payload(access, &request, Some(STORYBOARD_TIMEOUT))?;
    let text = model_response_json_text(access, &body)
        .ok_or_else(|| "Experimental storyboard response did not contain JSON.".to_owned())?;
    serde_json::from_str(&text)
        .map_err(|_| "Experimental storyboard JSON did not match the required schema.".to_owned())
}

pub(crate) fn validate_storyboard(
    content: &StoryboardContent,
    sources: &[StoryboardSource],
    brief: &str,
) -> Result<(), String> {
    if content.shots.is_empty() || content.shots.len() > 30 {
        return Err(storyboard_repair_message(
            "Storyboard must contain between 1 and 30 shots for safe local processing.",
            content.shots.iter().map(|shot| shot.order_index).collect(),
        ));
    }
    let total_duration = content
        .shots
        .iter()
        .map(|shot| shot.duration_ms)
        .sum::<i64>();
    if !(3_000..=120_000).contains(&content.target_duration_ms) {
        return Err(storyboard_repair_message(
            "Storyboard target duration must be between 3 and 120 seconds for safe local processing.",
            content.shots.iter().map(|shot| shot.order_index).collect(),
        ));
    }
    let target_tolerance = (content.target_duration_ms / 5).clamp(1_000, 8_000);
    if (total_duration - content.target_duration_ms).abs() > target_tolerance {
        return Err(storyboard_repair_message(
            "Storyboard shot durations must stay close to the model-proposed target duration.",
            content.shots.iter().map(|shot| shot.order_index).collect(),
        ));
    }
    if content.script_mode != "full_script" && content.script_mode != "key_message" {
        return Err(storyboard_repair_message(
            "Storyboard script mode is invalid.",
            content.shots.iter().map(|shot| shot.order_index).collect(),
        ));
    }
    if content.script_mode == "full_script" && total_duration < minimum_storyboard_duration(brief) {
        return Err(storyboard_repair_message(
            "Storyboard is too short for the supplied full-script narration.",
            content.shots.iter().map(|shot| shot.order_index).collect(),
        ));
    }
    if content.beats.is_empty() || content.beats.len() > 30 {
        return Err(storyboard_repair_message(
            "Storyboard must contain between 1 and 30 narrative beats for safe local processing.",
            content.shots.iter().map(|shot| shot.order_index).collect(),
        ));
    }
    let mut beat_ids = std::collections::HashSet::new();
    for beat in &content.beats {
        if beat.id.trim().is_empty()
            || beat.purpose.trim().is_empty()
            || beat.required_visual.trim().is_empty()
            || !beat_ids.insert(beat.id.as_str())
        {
            return Err(storyboard_repair_message(
                "Storyboard beats are invalid.",
                content.shots.iter().map(|shot| shot.order_index).collect(),
            ));
        }
    }
    let uncovered: std::collections::HashSet<&str> = content
        .uncovered_beat_ids
        .iter()
        .map(String::as_str)
        .collect();
    if uncovered.len() != content.uncovered_beat_ids.len()
        || uncovered.iter().any(|id| !beat_ids.contains(id))
    {
        return Err(storyboard_repair_message(
            "Storyboard uncovered beats are invalid.",
            content.shots.iter().map(|shot| shot.order_index).collect(),
        ));
    }
    let covered: std::collections::HashSet<&str> = content
        .shots
        .iter()
        .map(|shot| shot.beat_id.as_str())
        .collect();
    if beat_ids
        .iter()
        .any(|id| !covered.contains(id) && !uncovered.contains(id))
    {
        return Err(storyboard_repair_message(
            "Every storyboard beat must be covered or explicitly uncovered.",
            content.shots.iter().map(|shot| shot.order_index).collect(),
        ));
    }
    for (index, shot) in content.shots.iter().enumerate() {
        if shot.order_index != index as i64 + 1
            || shot.duration_ms <= 0
            || shot.purpose.trim().is_empty()
            || shot.reason.trim().is_empty()
            || shot.beat_id.trim().is_empty()
            || !beat_ids.contains(shot.beat_id.as_str())
            || !matches!(shot.match_level.as_str(), "direct" | "contextual")
        {
            return Err(storyboard_repair_message(
                "Storyboard shot fields are invalid.",
                vec![shot.order_index],
            ));
        }
        if uncovered.contains(shot.beat_id.as_str()) {
            return Err(storyboard_repair_message(
                "An uncovered beat cannot have a storyboard shot.",
                vec![shot.order_index],
            ));
        }
        let source = sources
            .iter()
            .find(|source| source.asset_id == shot.asset_id)
            .ok_or_else(|| {
                storyboard_repair_message(
                    "Storyboard referenced an unavailable asset.",
                    vec![shot.order_index],
                )
            })?;
        if source.kind == "video" {
            let duration = source.duration_ms.ok_or_else(|| {
                storyboard_repair_message(
                    "Storyboard referenced video without a verified duration.",
                    vec![shot.order_index],
                )
            })?;
            if shot.source_start_ms < 0
                || shot.source_end_ms <= shot.source_start_ms
                || shot.source_end_ms > duration
            {
                return Err(storyboard_repair_message(
                    "Storyboard referenced an invalid video time range.",
                    vec![shot.order_index],
                ));
            }
            if shot.duration_ms > shot.source_end_ms - shot.source_start_ms {
                return Err(storyboard_repair_message(
                    "Storyboard shot duration exceeds its verified video source range.",
                    vec![shot.order_index],
                ));
            }
        } else if source.kind != "image" || shot.source_start_ms != 0 || shot.source_end_ms != 0 {
            return Err(storyboard_repair_message(
                "Storyboard image references must use a zero source range.",
                vec![shot.order_index],
            ));
        }
    }
    validate_non_overlapping_video_sources(&content.shots, sources).map_err(|error| {
        storyboard_repair_message(
            error,
            content.shots.iter().map(|shot| shot.order_index).collect(),
        )
    })?;
    Ok(())
}

fn validate_non_overlapping_video_sources(
    shots: &[crate::models::StoryboardShot],
    sources: &[StoryboardSource],
) -> Result<(), String> {
    for (index, shot) in shots.iter().enumerate() {
        let is_video = sources
            .iter()
            .find(|source| source.asset_id == shot.asset_id)
            .is_some_and(|source| source.kind == "video");
        if !is_video {
            continue;
        }
        for other in shots.iter().skip(index + 1) {
            if shot.asset_id == other.asset_id
                && shot.source_start_ms < other.source_end_ms
                && other.source_start_ms < shot.source_end_ms
            {
                return Err(
                    "Storyboard cannot reuse overlapping video source ranges across beats."
                        .to_owned(),
                );
            }
        }
    }
    Ok(())
}

fn minimum_storyboard_duration(brief: &str) -> i64 {
    let word_count = brief
        .split_whitespace()
        .filter(|word| !word.is_empty())
        .count();
    if word_count < 20 {
        10_000
    } else {
        (word_count as i64 * 300).clamp(10_000, 45_000)
    }
}

fn normalize_storyboard_candidate(
    mut content: StoryboardContent,
    sources: &[StoryboardSource],
    brief: &str,
) -> StoryboardContent {
    let mut total_duration = 0_i64;
    for shot in &mut content.shots {
        if let Some(source) = sources
            .iter()
            .find(|source| source.asset_id == shot.asset_id)
        {
            if source.kind == "video" {
                let duration = source.duration_ms.unwrap_or(0).max(1);
                let desired_duration = shot.duration_ms.clamp(1, duration);
                let (mut start, mut end) = choose_storyboard_video_range(
                    source,
                    desired_duration,
                    shot.source_start_ms,
                    shot.source_end_ms,
                );
                if end - start < desired_duration {
                    let fallback_start = (duration.saturating_sub(desired_duration)) / 2;
                    start = fallback_start.clamp(0, duration.saturating_sub(1));
                    end = (start + desired_duration).min(duration).max(start + 1);
                }
                shot.source_start_ms = start;
                shot.source_end_ms = end;
                shot.duration_ms = (end - start).max(1);
            } else {
                shot.source_start_ms = 0;
                shot.source_end_ms = 0;
                shot.duration_ms = shot.duration_ms.max(1);
            }
        }
        total_duration += shot.duration_ms.max(1);
    }
    if total_duration > 0 {
        content.target_duration_ms = total_duration;
    }
    if content.script_mode == "full_script" && total_duration < minimum_storyboard_duration(brief) {
        content.script_mode = "key_message".to_owned();
    }
    content
}

fn choose_storyboard_video_range(
    source: &StoryboardSource,
    desired_duration: i64,
    preferred_start: i64,
    preferred_end: i64,
) -> (i64, i64) {
    let duration = source.duration_ms.unwrap_or(0).max(1);
    let desired_duration = desired_duration.clamp(1, duration);
    let mut segments = source
        .scene_segments
        .iter()
        .filter(|segment| segment.end_ms > segment.start_ms)
        .collect::<Vec<_>>();

    if segments.is_empty() {
        let max_start = duration.saturating_sub(desired_duration);
        let start = preferred_start.clamp(0, max_start);
        let end = (start + desired_duration).min(duration).max(start + 1);
        return (start, end);
    }

    let preferred_midpoint = if preferred_end > preferred_start {
        preferred_start + (preferred_end - preferred_start) / 2
    } else {
        preferred_start + desired_duration / 2
    };
    segments.sort_by_key(|segment| {
        let segment_duration = (segment.end_ms - segment.start_ms).max(1);
        let contains_preference =
            (segment.start_ms..=segment.end_ms).contains(&preferred_midpoint) as i64;
        let duration_penalty = if segment_duration >= desired_duration {
            segment_duration - desired_duration
        } else {
            desired_duration - segment_duration
        };
        let midpoint = segment.start_ms + segment_duration / 2;
        let midpoint_distance = (midpoint - preferred_midpoint).abs();
        (
            0_i64 - contains_preference,
            duration_penalty,
            midpoint_distance,
            segment.start_ms,
        )
    });

    let segment = segments[0];
    let segment_duration = (segment.end_ms - segment.start_ms).max(1);
    let clipped_duration = desired_duration.min(segment_duration).min(duration).max(1);
    let max_start = segment.end_ms.saturating_sub(clipped_duration);
    let mut start = preferred_start.clamp(segment.start_ms, max_start);
    if start < segment.start_ms || start > max_start {
        start = segment.start_ms + (segment_duration - clipped_duration) / 2;
    }
    start = start.clamp(segment.start_ms, max_start);
    let mut end = (start + clipped_duration).min(segment.end_ms).min(duration);
    if end <= start {
        start = segment.start_ms;
        end = (start + clipped_duration).min(segment.end_ms).min(duration);
    }
    if end <= start {
        end = (start + 1).min(duration);
    }
    (start, end)
}

fn fallback_storyboard(brief: &str, sources: &[StoryboardSource]) -> Option<StoryboardContent> {
    let usable_sources = sources.iter().take(8).collect::<Vec<_>>();
    if usable_sources.is_empty() {
        return None;
    }
    let beat_prefix = brief
        .split_whitespace()
        .take(6)
        .collect::<Vec<_>>()
        .join(" ");
    let mut beats = Vec::with_capacity(usable_sources.len());
    let mut shots = Vec::with_capacity(usable_sources.len());
    let mut total_duration = 0_i64;
    for (index, source) in usable_sources.iter().enumerate() {
        let beat_id = format!("beat-{}", index + 1);
        let shot_duration = match source.kind.as_str() {
            "video" => source.duration_ms.unwrap_or(5_000).clamp(3_000, 12_000),
            _ => 3_000,
        };
        let (source_start_ms, source_end_ms) = if source.kind == "video" {
            choose_storyboard_video_range(source, shot_duration, 0, shot_duration)
        } else {
            (0, 0)
        };
        beats.push(StoryboardBeat {
            id: beat_id.clone(),
            purpose: if index == 0 {
                format!("Introduce the request: {brief}")
            } else {
                format!("Support the next step with available media {index}")
            },
            required_visual: if index == 0 {
                format!("A visual introduction for {beat_prefix}")
            } else {
                format!("A supporting visual for available media {index}")
            },
        });
        shots.push(StoryboardShot {
            order_index: index as i64 + 1,
            duration_ms: shot_duration,
            purpose: if index == 0 {
                "Open with the user's requested topic.".to_owned()
            } else {
                "Continue the narrative with supporting footage.".to_owned()
            },
            on_screen_text: String::new(),
            asset_id: source.asset_id.clone(),
            source_start_ms,
            source_end_ms,
            reason: if source.kind == "video" {
                "This source is the best available safe segment for the topic.".to_owned()
            } else {
                "This still image supports the requested topic as a stable visual.".to_owned()
            },
            beat_id,
            match_level: if source.kind == "video" {
                "contextual".to_owned()
            } else {
                "direct".to_owned()
            },
        });
        total_duration += shot_duration;
    }
    let target_duration_ms = total_duration.max(minimum_storyboard_duration(brief));
    Some(StoryboardContent {
        brief: brief.to_owned(),
        title: if brief.is_empty() {
            "Auto storyboard".to_owned()
        } else {
            brief.chars().take(24).collect()
        },
        summary: format!(
            "Fallback storyboard built from {} available media sources.",
            usable_sources.len()
        ),
        target_duration_ms,
        script_mode: if total_duration < minimum_storyboard_duration(brief) {
            "key_message".to_owned()
        } else {
            "full_script".to_owned()
        },
        beats,
        uncovered_beat_ids: Vec::new(),
        shots,
    })
}

#[cfg(test)]
mod tests {
    use super::{minimum_storyboard_duration, validate_storyboard};
    use crate::models::{StoryboardBeat, StoryboardContent, StoryboardShot, StoryboardSource};

    fn source() -> StoryboardSource {
        StoryboardSource {
            asset_id: "asset-1".to_owned(),
            kind: "video".to_owned(),
            duration_ms: Some(10_000),
            scene_segments: Vec::new(),
            ocr_evidence: Vec::new(),
            visual_evidence: Vec::new(),
        }
    }

    fn content(match_level: &str) -> StoryboardContent {
        StoryboardContent {
            brief: "brief".to_owned(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 10_000,
            script_mode: "full_script".to_owned(),
            beats: vec![
                StoryboardBeat {
                    id: "context".to_owned(),
                    purpose: "Set the scene".to_owned(),
                    required_visual: "A verified product view".to_owned(),
                },
                StoryboardBeat {
                    id: "missing".to_owned(),
                    purpose: "Explain a hidden technical value".to_owned(),
                    required_visual: "Measured technical data".to_owned(),
                },
            ],
            uncovered_beat_ids: vec!["missing".to_owned()],
            shots: vec![StoryboardShot {
                order_index: 1,
                duration_ms: 10_000,
                purpose: "Set the scene".to_owned(),
                on_screen_text: String::new(),
                asset_id: "asset-1".to_owned(),
                source_start_ms: 0,
                source_end_ms: 10_000,
                reason: "The verified product view establishes context.".to_owned(),
                beat_id: "context".to_owned(),
                match_level: match_level.to_owned(),
            }],
        }
    }

    #[test]
    fn storyboard_can_honestly_leave_a_beat_uncovered() {
        assert!(validate_storyboard(&content("contextual"), &[source()], "brief").is_ok());
    }

    #[test]
    fn storyboard_rejects_an_insufficient_shot() {
        assert!(validate_storyboard(&content("insufficient"), &[source()], "brief").is_err());
    }

    #[test]
    fn storyboard_rejects_overlapping_video_ranges() {
        let mut storyboard = content("direct");
        storyboard.shots.push(StoryboardShot {
            order_index: 2,
            duration_ms: 5_000,
            purpose: "Repeat the same source".to_owned(),
            on_screen_text: String::new(),
            asset_id: "asset-1".to_owned(),
            source_start_ms: 5_000,
            source_end_ms: 10_000,
            reason: "This deliberately overlaps the first test shot.".to_owned(),
            beat_id: "context".to_owned(),
            match_level: "direct".to_owned(),
        });
        assert!(validate_storyboard(&storyboard, &[source()], "brief").is_err());
    }

    #[test]
    fn long_english_briefs_receive_a_reading_duration_floor() {
        let brief = std::iter::repeat("word")
            .take(120)
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(minimum_storyboard_duration(&brief), 36_000);
    }

    #[test]
    fn key_message_storyboard_can_be_shorter_than_full_narration() {
        let brief = std::iter::repeat("word")
            .take(120)
            .collect::<Vec<_>>()
            .join(" ");
        let mut storyboard = content("direct");
        storyboard.script_mode = "key_message".to_owned();
        assert!(validate_storyboard(&storyboard, &[source()], &brief).is_ok());
    }
}

#[tauri::command]
pub fn generate_storyboard(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    brief: String,
) -> Result<StoryboardVersion, String> {
    generate_storyboard_internal(app, project_id, editing_task_id, brief, true)
}

/// Agent storyboard generation consumes only analysis evidence already ready
/// in the scoped project. Starting or reprioritizing analysis remains an
/// explicit `request_asset_analysis` tool decision.
pub(crate) fn generate_storyboard_for_agent(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    brief: String,
) -> Result<StoryboardVersion, String> {
    generate_storyboard_internal(app, project_id, editing_task_id, brief, false)
}

fn generate_storyboard_internal(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    brief: String,
    schedule_visual_analysis: bool,
) -> Result<StoryboardVersion, String> {
    log::info!("Starting AI storyboard generation.");
    let brief = brief.trim();
    if brief.is_empty() {
        return Err("Storyboard brief cannot be empty.".to_owned());
    }
    let connection = open_connection(&app)?;
    let task_exists = connection
        .query_row(
            "SELECT COUNT(*) FROM editing_tasks WHERE id = ?1 AND project_id = ?2",
            params![editing_task_id, project_id],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|error| error.to_string())?;
    if task_exists != 1 {
        return Err("Editing task does not belong to this project.".to_owned());
    }
    if schedule_visual_analysis {
        let priority_batch = prioritize_pending_visual_batches(&app, &project_id, brief)?;
        wait_for_visual_batch(&app, priority_batch.as_deref())?;
    }
    let (sources, visual_ready_count) = storyboard_sources(&connection, &project_id)?;
    if sources.is_empty() {
        return if visual_ready_count == 0 {
            Err("storyboard_visual_evidence_unavailable: visual_ready_candidates=0".to_owned())
        } else {
            Err(format!(
                "storyboard_source_inventory_unavailable: visual_ready_candidates={visual_ready_count}; accessible_source_files=0"
            ))
        };
    }
    let access = ModelAccess::resolve().map_err(|error| {
        log::warn!("AI storyboard generation could not access the configured provider.");
        error
    })?;
    let mut previous = None;
    let mut feedback = None;
    let mut content = None;
    for _ in 0..MAX_STORYBOARD_REVISIONS {
        match request_storyboard(
            &access,
            brief,
            &sources,
            previous.as_ref(),
            feedback.as_deref(),
        ) {
            Ok(candidate) => match validate_storyboard(&candidate, &sources, brief) {
                Ok(()) => {
                    content = Some(candidate);
                    break;
                }
                Err(error) => {
                    log::warn!("AI storyboard validation failed.");
                    feedback = Some(error);
                    previous = Some(candidate);
                }
            },
            Err(error) => {
                log::warn!("AI storyboard request failed.");
                feedback = Some(error);
                previous = None;
            }
        }
    }
    let content = content
        .map(|candidate| normalize_storyboard_candidate(candidate, &sources, brief))
        .or_else(|| fallback_storyboard(brief, &sources))
        .ok_or_else(|| {
            feedback.unwrap_or_else(|| {
                "Storyboard generation did not produce a valid result.".to_owned()
            })
        })?;
    let version_number = connection.query_row(
        "SELECT COALESCE(MAX(version_number), 0) + 1 FROM storyboard_versions WHERE project_id = ?1",
        params![project_id], |row| row.get::<_, i64>(0),
    ).map_err(|error| error.to_string())?;
    let version = StoryboardVersion {
        id: Uuid::new_v4().to_string(),
        project_id,
        editing_task_id: editing_task_id.clone(),
        version_number,
        brief: brief.to_owned(),
        title: content.title,
        summary: content.summary,
        target_duration_ms: content.target_duration_ms,
        script_mode: content.script_mode.clone(),
        beats: content.beats.clone(),
        uncovered_beat_ids: content.uncovered_beat_ids.clone(),
        shots: content.shots,
        created_at: now_millis(),
    };
    connection.execute(
        "INSERT INTO storyboard_versions (id, project_id, editing_task_id, version_number, status, content_json, created_at) VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?6)",
        params![version.id, version.project_id, version.editing_task_id, version.version_number, serde_json::to_string(&StoryboardContent { brief: version.brief.clone(), title: version.title.clone(), summary: version.summary.clone(), target_duration_ms: content.target_duration_ms, script_mode: content.script_mode.clone(), beats: version.beats.clone(), uncovered_beat_ids: version.uncovered_beat_ids.clone(), shots: version.shots.clone() }).map_err(|error| error.to_string())?, version.created_at],
    ).map_err(|error| error.to_string())?;
    connection
        .execute(
            "UPDATE editing_tasks SET brief = ?1, title = CASE WHEN title IN ('新的剪辑任务', '新的剪辑会话') THEN substr(?1, 1, 28) ELSE title END, updated_at = ?2 WHERE id = ?3",
            params![brief, now_millis(), editing_task_id],
        )
        .map_err(|error| error.to_string())?;
    log::info!("Completed AI storyboard generation.");
    Ok(version)
}

#[tauri::command]
pub fn get_latest_storyboard(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
) -> Result<Option<StoryboardVersion>, String> {
    let connection = open_connection(&app)?;
    connection.query_row(
        "SELECT id, version_number, content_json, created_at FROM storyboard_versions WHERE project_id = ?1 AND editing_task_id = ?2 ORDER BY version_number DESC LIMIT 1",
        params![project_id, editing_task_id],
        |row| {
            let content: StoryboardContent = serde_json::from_str(&row.get::<_, String>(2)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(StoryboardVersion {
                id: row.get(0)?,
                project_id: project_id.clone(),
                editing_task_id: editing_task_id.clone(),
                version_number: row.get(1)?,
                brief: content.brief,
                title: content.title,
                summary: content.summary,
                target_duration_ms: content.target_duration_ms,
                script_mode: content.script_mode,
                beats: content.beats,
                uncovered_beat_ids: content.uncovered_beat_ids,
                shots: content.shots,
                created_at: row.get(3)?,
            })
        },
    ).optional().map_err(|_| "Storyboard version could not be read.".to_owned())
}

pub(crate) fn load_storyboard_version(
    connection: &Connection,
    storyboard_version_id: &str,
) -> Result<StoryboardVersion, String> {
    connection.query_row(
        "SELECT id, project_id, editing_task_id, version_number, content_json, created_at FROM storyboard_versions WHERE id = ?1",
        params![storyboard_version_id],
        |row| {
            let content: StoryboardContent = serde_json::from_str(&row.get::<_, String>(4)?)
                .map_err(|_| rusqlite::Error::InvalidQuery)?;
            Ok(StoryboardVersion {
                id: row.get(0)?, project_id: row.get(1)?, editing_task_id: row.get(2)?, version_number: row.get(3)?, brief: content.brief,
                title: content.title, summary: content.summary, target_duration_ms: content.target_duration_ms, script_mode: content.script_mode, beats: content.beats, uncovered_beat_ids: content.uncovered_beat_ids, shots: content.shots, created_at: row.get(5)?,
            })
        },
    ).map_err(|_| "Storyboard version could not be read.".to_owned())
}
