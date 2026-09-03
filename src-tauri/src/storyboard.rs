//! 只使用已就绪真实媒体证据生成版本化 storyboard，并校验证据源时间范围。
//! 文件名和路径只能用于本地组织，不能冒充媒体内容证据。

mod keyframes;
pub(crate) mod multimodal;
pub(crate) mod phases;
pub(crate) mod repair;
mod scoring;
pub(crate) mod semantic;
mod validation;

use crate::storyboard::repair::{RepairPacket, StoryboardIssue};

use crate::assets::{prioritize_pending_visual_batches, wait_for_visual_batch};
use crate::db::{now_millis, open_connection};
use crate::models::{
    StoryboardContent, StoryboardSource, StoryboardVersion, TechnicalMetadata, TimelineContent,
};
use crate::provider::{model_response_json_text, post_model_payload, ModelAccess};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;
use tauri::AppHandle;
use uuid::Uuid;

/// Timeout for a single storyboard generation model request so a slow or hung
/// provider never blocks the agent loop forever.
const STORYBOARD_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_STORYBOARD_REVISIONS: usize = 3;
const MAX_PHASE1_REVISIONS: usize = 3;
const MAX_BEAT_SPOKEN_MS: i64 = 8_000;

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
        // Top-12 是视觉镜头候选，不以非视频素材补足数量；模型只在技术分析完成的
        // 可访问视频中判断语义与画面优先级。
        "SELECT id, kind, metadata_json, source_reference FROM assets WHERE project_id = ?1 AND analysis_status = 'ready' AND kind = 'video' AND coalesce((SELECT excluded FROM asset_user_metadata um WHERE um.asset_id = assets.id), 0) = 0",
    ).map_err(|error| error.to_string())?;
    let rows = statement
        .query_map(params![project_id], |row| {
            let metadata: TechnicalMetadata =
                serde_json::from_str(&row.get::<_, String>(2)?).unwrap_or_default();
            let visual_ready =
                metadata.visual_analysis_status == "ready" && !metadata.visual_evidence.is_empty();
            let source_available = Path::new(&row.get::<_, String>(3)?).is_file();

            let visual_quality_score = metadata.visual_quality_score.or_else(|| {
                let scores = metadata
                    .scene_segments
                    .iter()
                    .filter_map(|segment| segment.visual_quality_score)
                    .collect::<Vec<_>>();
                (!scores.is_empty()).then(|| scores.iter().sum::<f64>() / scores.len() as f64)
            });
            let evidence_embedding = semantic::embedding_is_current(&metadata)
                .then(|| metadata.evidence_embedding.clone())
                .flatten();

            // 从元数据中提取关键帧网格图路径
            let keyframe_grid_path = metadata.keyframe_grid_path.clone();

            Ok((
                StoryboardSource {
                    asset_id: row.get(0)?,
                    kind: row.get(1)?,
                    duration_ms: metadata.duration_ms,
                    scene_segments: metadata
                        .scene_segments
                        .into_iter()
                        .map(|mut seg| {
                            // 场景段时长从端点推算
                            seg.scene_duration_ms = Some(seg.end_ms - seg.start_ms);
                            seg
                        })
                        .collect(),
                    ocr_evidence: metadata.ocr_evidence,
                    visual_evidence: metadata.visual_evidence,
                    visual_quality_score,
                    evidence_embedding,
                    keyframe_grid_path,
                },
                visual_ready,
                source_available,
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
            .filter_map(|(source, _, source_available)| source_available.then_some(source))
            .collect(),
        visual_ready_count,
    ))
}

fn storyboard_usage_counts(
    connection: &Connection,
    project_id: &str,
) -> Result<HashMap<String, i32>, String> {
    let mut statement = connection
        .prepare(
            "WITH ranked AS (
                SELECT timeline.content_json,
                       ROW_NUMBER() OVER (
                           PARTITION BY storyboard.editing_task_id
                           ORDER BY timeline.created_at DESC, timeline.version_number DESC
                       ) AS row_number
                FROM timeline_versions timeline
                JOIN storyboard_versions storyboard ON storyboard.id = timeline.storyboard_version_id
                WHERE timeline.project_id = ?1 AND storyboard.editing_task_id IS NOT NULL
             )
             SELECT content_json FROM ranked WHERE row_number = 1",
        )
        .map_err(|_| "Storyboard usage history could not be read.".to_owned())?;
    let rows = statement
        .query_map(params![project_id], |row| row.get::<_, String>(0))
        .map_err(|_| "Storyboard usage history could not be read.".to_owned())?;
    let mut usage_counts = HashMap::new();
    for content_json in rows {
        let Ok(content_json) = content_json else {
            log::warn!("Skipped unreadable storyboard usage history row.");
            continue;
        };
        let Ok(content) = serde_json::from_str::<TimelineContent>(&content_json) else {
            log::warn!("Skipped invalid storyboard usage history JSON.");
            continue;
        };
        let unique_assets = content
            .clips
            .into_iter()
            .map(|clip| clip.asset_id)
            .collect::<HashSet<_>>();
        for asset_id in unique_assets {
            *usage_counts.entry(asset_id).or_insert(0) += 1;
        }
    }
    Ok(usage_counts)
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
    if content.script_mode == "full_script" {
        let estimated_duration = minimum_storyboard_duration(brief);
        // 镜头数下限按未截断的朗读时长计算，避免短文案被强制拆成多个镜头。
        let raw_estimated_duration = estimated_storyboard_duration_ms(brief);
        let minimum_shot_count = ((raw_estimated_duration + 7_999) / 8_000).max(1) as usize;
        if content.shots.len() < minimum_shot_count {
            return Err(storyboard_repair_message(
                format!(
                    "Storyboard has too few shots for the supplied full-script narration. Estimated narration duration is about {} ms, so the storyboard should contain at least {} shots to keep each shot near 8 seconds or less.",
                    raw_estimated_duration,
                    minimum_shot_count
                ),
                content.shots.iter().map(|shot| shot.order_index).collect(),
            ));
        }
        if total_duration < estimated_duration {
            return Err(storyboard_repair_message(
                format!(
                    "Storyboard is too short for the supplied full-script narration. Estimated narration duration is about {} ms.",
                    estimated_duration
                ),
                content.shots.iter().map(|shot| shot.order_index).collect(),
            ));
        }
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
        if estimated_storyboard_duration_ms(&beat.narration) > MAX_BEAT_SPOKEN_MS {
            return Err(storyboard_repair_message(
                format!(
                    "Beat '{}' narration reads for about {} ms, exceeding the {} ms per-beat limit; split it into shorter beats.",
                    beat.id,
                    estimated_storyboard_duration_ms(&beat.narration),
                    MAX_BEAT_SPOKEN_MS
                ),
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
    validate_shot_diversity(&content.shots).map_err(|error| {
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
                log::warn!(
                    "Overlapping video source range detected: asset_id={}, shot_{}=[{}-{}]ms, shot_{}=[{}-{}]ms",
                    shot.asset_id,
                    shot.order_index,
                    shot.source_start_ms,
                    shot.source_end_ms,
                    other.order_index,
                    other.source_start_ms,
                    other.source_end_ms
                );
                return Err(
                    "Storyboard cannot reuse overlapping video source ranges across beats."
                        .to_owned(),
                );
            }
        }
    }
    Ok(())
}

/// 校验镜头多样性：禁止连续使用同一素材，且同一素材占比不得超过 40%。
fn validate_shot_diversity(shots: &[crate::models::StoryboardShot]) -> Result<(), String> {
    if shots.len() < 2 {
        return Ok(());
    }

    // 检查连续镜头是否使用同一素材
    for window in shots.windows(2) {
        if window[0].asset_id == window[1].asset_id {
            log::warn!(
                "Consecutive shots use same asset: shot_{}={}, shot_{}={}",
                window[0].order_index,
                window[0].asset_id,
                window[1].order_index,
                window[1].asset_id
            );
            return Err(format!(
                "Consecutive shots (index {} and {}) cannot use the same asset. Choose different footage to maintain visual variety. Try alternating between available assets or selecting non-adjacent time ranges from this asset.",
                window[0].order_index,
                window[1].order_index
            ));
        }
    }

    // 检查单一素材占比
    let mut asset_usage = std::collections::HashMap::new();
    for shot in shots {
        *asset_usage.entry(&shot.asset_id).or_insert(0) += 1;
    }

    let max_allowed = max_asset_uses_for_shot_count(shots.len());
    for (asset_id, count) in asset_usage {
        if count > max_allowed {
            let percentage = count * 100 / shots.len();
            log::warn!(
                "Asset usage exceeds diversity limit: asset_id={}, usage={}/{} shots ({}%), limit=40% ({} shots)",
                asset_id,
                count,
                shots.len(),
                percentage,
                max_allowed
            );
            return Err(format!(
                "Asset '{}' appears in {} of {} shots ({}%), exceeding the 40% diversity limit. Recommended: use this asset for at most {} shots and distribute remaining shots across other available footage. Consider replacing repetitive shots with contextually similar scenes from different assets.",
                asset_id,
                count,
                shots.len(),
                percentage,
                max_allowed
            ));
        }
    }

    Ok(())
}

fn max_asset_uses_for_shot_count(shot_count: usize) -> usize {
    (shot_count * 2 / 5).max(1)
}

fn estimated_english_words(text: &str) -> f64 {
    let mut words: f64 = 0.0;
    let mut ascii_run = false;
    let mut cjk_count: f64 = 0.0;
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            if cjk_count > 0.0 {
                words += (cjk_count / 2.0).ceil();
                cjk_count = 0.0;
            }
            if !ascii_run {
                words += 1.0;
                ascii_run = true;
            }
        } else if ('\u{4e00}'..='\u{9fff}').contains(&character) {
            ascii_run = false;
            cjk_count += 1.0;
            if cjk_count >= 2.0 {
                words += 1.0;
                cjk_count = 0.0;
            }
        } else {
            ascii_run = false;
            if cjk_count > 0.0 {
                words += (cjk_count / 2.0).ceil();
                cjk_count = 0.0;
            }
        }
    }
    if cjk_count > 0.0 {
        words += (cjk_count / 2.0).ceil();
    }
    words.max(1.0)
}

fn estimated_storyboard_duration_ms(brief: &str) -> i64 {
    let words = estimated_english_words(brief);
    (words * 300.0).round() as i64
}

fn minimum_storyboard_duration(brief: &str) -> i64 {
    estimated_storyboard_duration_ms(brief).clamp(10_000, 120_000)
}

/// 从口播文本生成简短字幕：取第一句，最多 40 个字符。
/// 模型漏写 onScreenText 时作为兜底，保证成片有可见字幕。
fn subtitle_text_from_narration(narration: &str) -> String {
    let first_sentence = narration
        .split(|character: char| character == '。' || character == '！' || character == '？')
        .map(str::trim)
        .find(|part| !part.is_empty())
        .unwrap_or(narration.trim());
    first_sentence.chars().take(40).collect()
}

fn normalize_storyboard_candidate(
    mut content: StoryboardContent,
    sources: &[StoryboardSource],
    brief: &str,
) -> StoryboardContent {
    log::info!(
        "Normalizing storyboard candidate: shots={}, initial_target_duration_ms={}",
        content.shots.len(),
        content.target_duration_ms
    );
    let mut corrections = 0;
    for shot in &mut content.shots {
        if let Some(source) = sources
            .iter()
            .find(|source| source.asset_id == shot.asset_id)
        {
            if source.kind == "video" {
                let original_start = shot.source_start_ms;
                let original_end = shot.source_end_ms;
                let duration = source.duration_ms.unwrap_or(0).max(1);
                let preferred_span = (original_end - original_start).max(1);
                let desired_duration = shot.duration_ms.min(preferred_span).clamp(1, duration);
                let range_already_valid = original_start >= 0
                    && original_end > original_start
                    && original_end <= duration
                    && desired_duration <= original_end - original_start;
                let (start, end) = if range_already_valid {
                    (original_start, original_end)
                } else {
                    choose_storyboard_video_range(
                        source,
                        desired_duration,
                        original_start,
                        original_end,
                    )
                };
                if start != original_start || end != original_end {
                    corrections += 1;
                    log::info!(
                        "Corrected video range for shot_{}: asset_id={}, [{}-{}]ms -> [{}-{}]ms",
                        shot.order_index,
                        shot.asset_id,
                        original_start,
                        original_end,
                        start,
                        end
                    );
                }
                shot.source_start_ms = start;
                shot.source_end_ms = end;
                shot.duration_ms = (end - start).min(desired_duration).max(1);
            } else {
                shot.source_start_ms = 0;
                shot.source_end_ms = 0;
                shot.duration_ms = shot.duration_ms.max(1);
            }
        }
    }
    resolve_overlapping_video_ranges(&mut content.shots, sources);
    content
        .uncovered_beat_ids
        .retain(|beat_id| !content.shots.iter().any(|shot| shot.beat_id == *beat_id));
    for (index, shot) in content.shots.iter_mut().enumerate() {
        shot.order_index = index as i64 + 1;
    }
    // 按 beat 分组补全子镜头字段：同一 beat 的连续 shot 重新编号并标注角色，
    // 保证模型未返回这些字段时数据也自洽。
    let mut group_cursor = 0usize;
    while group_cursor < content.shots.len() {
        let group_beat_id = content.shots[group_cursor].beat_id.clone();
        let mut group_end = group_cursor;
        while group_end < content.shots.len()
            && content.shots[group_end].beat_id == group_beat_id
        {
            group_end += 1;
        }
        let group_count = (group_end - group_cursor) as i64;
        for (offset, shot) in content
            .shots
            .iter_mut()
            .enumerate()
            .take(group_end)
            .skip(group_cursor)
        {
            let part_index = (offset - group_cursor + 1) as i64;
            shot.beat_part_index = part_index;
            shot.beat_part_count = group_count;
            shot.split_role = if group_count <= 1 || part_index == 1 {
                "lead".to_owned()
            } else if part_index == group_count {
                "tail".to_owned()
            } else {
                "bridge".to_owned()
            };
        }
        group_cursor = group_end;
    }
    let beat_narration = content
        .beats
        .iter()
        .map(|beat| {
            let spoken = if !beat.narration.trim().is_empty() {
                beat.narration.trim().to_owned()
            } else {
                beat.purpose.trim().to_owned()
            };
            (beat.id.clone(), spoken)
        })
        .collect::<std::collections::HashMap<_, _>>();
    for shot in &mut content.shots {
        if shot.narration_text.trim().is_empty() {
            shot.narration_text = beat_narration
                .get(&shot.beat_id)
                .cloned()
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| shot.purpose.trim().to_owned());
        }
        if shot.on_screen_text.trim().is_empty() && !shot.narration_text.trim().is_empty() {
            shot.on_screen_text = subtitle_text_from_narration(&shot.narration_text);
        }
    }
    let total_duration: i64 = content
        .shots
        .iter()
        .map(|shot| shot.duration_ms.max(1))
        .sum();
    if total_duration > 0 {
        content.target_duration_ms = total_duration;
    }
    if content.script_mode == "full_script" && total_duration < minimum_storyboard_duration(brief) {
        content.script_mode = "key_message".to_owned();
        log::info!(
            "Downgraded script mode: full_script -> key_message (total_duration={}ms < minimum={}ms)",
            total_duration,
            minimum_storyboard_duration(brief)
        );
    }
    log::info!(
        "Normalization complete: corrections={}, final_duration={}ms, script_mode={}",
        corrections,
        total_duration,
        content.script_mode
    );
    content
}

fn resolve_overlapping_video_ranges(
    shots: &mut [crate::models::StoryboardShot],
    sources: &[StoryboardSource],
) {
    let mut used: std::collections::HashMap<String, Vec<(i64, i64)>> =
        std::collections::HashMap::new();
    for shot in shots.iter_mut() {
        let Some(source) = sources
            .iter()
            .find(|source| source.asset_id == shot.asset_id)
        else {
            continue;
        };
        if source.kind != "video" {
            continue;
        }
        let duration = source.duration_ms.unwrap_or(0).max(1);
        let need = shot.duration_ms.clamp(1, duration);
        let occupied = used.entry(shot.asset_id.clone()).or_default();
        let mut start = shot.source_start_ms.max(0);
        let mut end = shot.source_end_ms.min(duration).max(start + 1);
        if ranges_overlap(start, end, occupied) {
            if let Some((free_start, free_end)) = find_free_window(duration, need, occupied, start)
            {
                start = free_start;
                end = free_end;
            } else if let Some((free_start, free_end)) =
                find_free_window(duration, 1, occupied, start)
            {
                start = free_start;
                end = free_end;
            }
        }
        shot.source_start_ms = start;
        shot.source_end_ms = end.min(duration).max(start + 1);
        shot.duration_ms = (shot.source_end_ms - shot.source_start_ms).min(need).max(1);
        occupied.push((shot.source_start_ms, shot.source_end_ms));
    }
    let asset_ids = used.keys().cloned().collect::<Vec<_>>();
    for asset_id in asset_ids {
        let duration = sources
            .iter()
            .find(|source| source.asset_id == asset_id)
            .and_then(|source| source.duration_ms)
            .unwrap_or(0)
            .max(1);
        if video_asset_ranges_overlap(shots, &asset_id) {
            pack_video_asset_shots(shots, &asset_id, duration);
        }
    }
}

fn video_asset_ranges_overlap(shots: &[crate::models::StoryboardShot], asset_id: &str) -> bool {
    let ranges = shots
        .iter()
        .filter(|shot| shot.asset_id == asset_id)
        .map(|shot| (shot.source_start_ms, shot.source_end_ms))
        .collect::<Vec<_>>();
    ranges.iter().enumerate().any(|(index, (start, end))| {
        ranges
            .iter()
            .skip(index + 1)
            .any(|(other_start, other_end)| start < other_end && other_start < end)
    })
}

fn pack_video_asset_shots(
    shots: &mut [crate::models::StoryboardShot],
    asset_id: &str,
    duration: i64,
) {
    let mut indices = shots
        .iter()
        .enumerate()
        .filter(|(_, shot)| shot.asset_id == asset_id)
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if indices.len() < 2 {
        return;
    }
    indices.sort_by_key(|&index| shots[index].source_start_ms);
    let slice = (duration / indices.len() as i64).max(1);
    let last = indices.len() - 1;
    for (offset, &index) in indices.iter().enumerate() {
        let start = offset as i64 * slice;
        let end = if offset == last {
            duration
        } else {
            (start + slice).min(duration)
        };
        shots[index].source_start_ms = start.min(duration.saturating_sub(1));
        shots[index].source_end_ms = end.max(shots[index].source_start_ms + 1).min(duration);
        shots[index].duration_ms =
            (shots[index].source_end_ms - shots[index].source_start_ms).max(1);
    }
}

fn ranges_overlap(start: i64, end: i64, used: &[(i64, i64)]) -> bool {
    used.iter()
        .any(|(used_start, used_end)| start < *used_end && *used_start < end)
}

fn find_free_window(
    duration: i64,
    need: i64,
    used: &[(i64, i64)],
    preferred_start: i64,
) -> Option<(i64, i64)> {
    let mut intervals = used.to_vec();
    intervals.sort_by_key(|item| item.0);
    let mut cursor = 0_i64;
    let mut gaps = Vec::new();
    for (start, end) in intervals {
        if start > cursor {
            gaps.push((cursor, start));
        }
        cursor = cursor.max(end);
    }
    if cursor < duration {
        gaps.push((cursor, duration));
    }
    let need = need.clamp(1, duration);
    gaps.iter()
        .filter(|(start, end)| end - start >= need)
        .min_by_key(|(start, _)| (*start - preferred_start).abs())
        .map(|(start, end)| {
            let placed = preferred_start.clamp(*start, end - need);
            (placed, placed + need)
        })
        .or_else(|| {
            gaps.iter()
                .max_by_key(|(start, end)| end - start)
                .filter(|(start, end)| end - start > 1)
                .map(|(start, end)| (*start, *end))
        })
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

#[cfg(test)]
mod tests {
    use super::{
        estimated_storyboard_duration_ms, minimum_storyboard_duration,
        normalize_storyboard_candidate, storyboard_sources, storyboard_usage_counts,
        validate_storyboard,
    };
    use crate::models::{
        StoryboardBeat, StoryboardContent, StoryboardShot, StoryboardSource, TimelineClip,
        TimelineContent,
    };
    use rusqlite::{params, Connection};
    use std::fs;
    use uuid::Uuid;

    fn source() -> StoryboardSource {
        StoryboardSource {
            asset_id: "asset-1".to_owned(),
            kind: "video".to_owned(),
            duration_ms: Some(10_000),
            scene_segments: Vec::new(),
            ocr_evidence: Vec::new(),
            visual_evidence: Vec::new(),
            visual_quality_score: None,
            evidence_embedding: None,
            keyframe_grid_path: None,
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
                    narration: "This is the opening scene.".to_owned(),
                },
                StoryboardBeat {
                    id: "missing".to_owned(),
                    purpose: "Explain a hidden technical value".to_owned(),
                    required_visual: "Measured technical data".to_owned(),
                    narration: String::new(),
                },
            ],
            uncovered_beat_ids: vec!["missing".to_owned()],
            shots: vec![StoryboardShot {
                order_index: 1,
                duration_ms: 10_000,
                purpose: "Set the scene".to_owned(),
                on_screen_text: String::new(),
                narration_text: String::new(),
                asset_id: "asset-1".to_owned(),
                source_start_ms: 0,
                source_end_ms: 10_000,
                reason: "The verified product view establishes context.".to_owned(),
                beat_id: "context".to_owned(),
                match_level: match_level.to_owned(),
                beat_part_index: 1,
                beat_part_count: 1,
                split_role: "lead".to_owned(),
            }],
        }
    }

    fn timeline_content(asset_ids: &[&str]) -> String {
        let clips = asset_ids
            .iter()
            .enumerate()
            .map(|(index, asset_id)| TimelineClip {
                shot_index: index as i64 + 1,
                asset_id: (*asset_id).to_owned(),
                source_start_ms: 0,
                source_end_ms: 1_000,
                timeline_start_ms: index as i64 * 1_000,
                timeline_end_ms: (index as i64 + 1) * 1_000,
                on_screen_text: String::new(),
                ..Default::default()
            })
            .collect();
        serde_json::to_string(&TimelineContent {
            clips,
            text_tracks: Vec::new(),
            music_tracks: Vec::new(),
            voiceover_tracks: Vec::new(),
            overlay_clips: Vec::new(),
            quality_report: None,
        })
        .expect("serialize timeline fixture")
    }

    #[test]
    fn storyboard_sources_only_returns_ready_accessible_videos() {
        let connection = Connection::open_in_memory().expect("open test database");
        connection
            .execute_batch(
                "CREATE TABLE assets (
                    id TEXT PRIMARY KEY,
                    project_id TEXT NOT NULL,
                    kind TEXT NOT NULL,
                    metadata_json TEXT NOT NULL,
                    source_reference TEXT NOT NULL,
                    analysis_status TEXT NOT NULL
                );
                CREATE TABLE asset_user_metadata (
                    asset_id TEXT PRIMARY KEY,
                    excluded INTEGER NOT NULL DEFAULT 0
                );",
            )
            .expect("create source fixture tables");
        let directory = std::env::temp_dir().join(format!(
            "assembly-storyboard-source-test-{}",
            Uuid::new_v4()
        ));
        fs::create_dir_all(&directory).expect("create source fixture directory");
        let available = directory.join("available.bin");
        fs::write(&available, b"fixture").expect("create accessible source fixture");
        let available = available.to_string_lossy().into_owned();
        let missing = directory.join("missing.bin").to_string_lossy().into_owned();
        let ready_metadata = r#"{"durationMs":10000,"width":1920,"height":1080,"fps":30.0,"hasAudio":false,"thumbnailPath":null,"keyframes":[],"sceneSegments":[],"ocrEvidence":[],"visualEvidence":[{"timeMs":0,"subjects":["factory"],"scene":"factory floor","actions":[],"products":[],"qualityNotes":[]}],"visualAnalysisNote":null,"visualAnalysisStatus":"ready","visualQualityScore":0.82,"keyframeGridPath":null}"#;

        for (id, kind, status, source) in [
            ("ready-video", "video", "ready", available.as_str()),
            ("queued-video", "video", "queued", available.as_str()),
            ("ready-image", "image", "ready", available.as_str()),
            ("ready-audio", "audio", "ready", available.as_str()),
            ("missing-video", "video", "ready", missing.as_str()),
            ("excluded-video", "video", "ready", available.as_str()),
        ] {
            connection
                .execute(
                    "INSERT INTO assets (id, project_id, kind, metadata_json, source_reference, analysis_status) VALUES (?1, 'project-1', ?2, ?3, ?4, ?5)",
                    params![id, kind, ready_metadata, source, status],
                )
                .expect("insert source fixture");
        }
        connection
            .execute(
                "INSERT INTO asset_user_metadata (asset_id, excluded) VALUES ('excluded-video', 1)",
                [],
            )
            .expect("exclude source fixture");

        let (sources, visual_ready_count) =
            storyboard_sources(&connection, "project-1").expect("load storyboard sources");

        assert_eq!(
            sources
                .iter()
                .map(|source| source.asset_id.as_str())
                .collect::<Vec<_>>(),
            ["ready-video"]
        );
        // 诊断计数在文件可访问性过滤前计算，因此还包含 missing-video。
        assert_eq!(visual_ready_count, 2);
        assert_eq!(sources[0].visual_quality_score, Some(0.82));
        fs::remove_file(directory.join("available.bin")).expect("remove source fixture file");
        fs::remove_dir(&directory).expect("remove source fixture directory");
    }

    #[test]
    fn usage_counts_only_the_latest_timeline_once_per_editing_task() {
        let connection = Connection::open_in_memory().expect("open test database");
        connection
            .execute_batch(
                "CREATE TABLE storyboard_versions (
                    id TEXT PRIMARY KEY,
                    project_id TEXT NOT NULL,
                    editing_task_id TEXT
                );
                CREATE TABLE timeline_versions (
                    id TEXT PRIMARY KEY,
                    project_id TEXT NOT NULL,
                    storyboard_version_id TEXT NOT NULL,
                    version_number INTEGER NOT NULL,
                    content_json TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                );",
            )
            .expect("create usage fixture tables");
        for (storyboard_id, task_id) in [
            ("story-1", "task-1"),
            ("story-2", "task-2"),
            ("story-3", "task-3"),
        ] {
            connection
                .execute(
                    "INSERT INTO storyboard_versions VALUES (?1, 'project-1', ?2)",
                    params![storyboard_id, task_id],
                )
                .expect("insert storyboard fixture");
        }
        for (id, storyboard_id, version, created_at, assets) in [
            ("old", "story-1", 1, 1, timeline_content(&["asset-a"])),
            (
                "latest",
                "story-1",
                2,
                2,
                timeline_content(&["asset-a", "asset-a", "asset-b"]),
            ),
            (
                "other-task",
                "story-2",
                3,
                3,
                timeline_content(&["asset-a"]),
            ),
            ("invalid", "story-3", 1, 4, "not-json".to_owned()),
        ] {
            connection
                .execute(
                    "INSERT INTO timeline_versions VALUES (?1, 'project-1', ?2, ?3, ?4, ?5)",
                    params![id, storyboard_id, version, assets, created_at],
                )
                .expect("insert timeline fixture");
        }

        let counts = storyboard_usage_counts(&connection, "project-1").expect("count usage");
        assert_eq!(counts.get("asset-a"), Some(&2));
        assert_eq!(counts.get("asset-b"), Some(&1));
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
            narration_text: String::new(),
            asset_id: "asset-1".to_owned(),
            source_start_ms: 5_000,
            source_end_ms: 10_000,
            reason: "This deliberately overlaps the first test shot.".to_owned(),
            beat_id: "context".to_owned(),
            match_level: "direct".to_owned(),
            beat_part_index: 1,
            beat_part_count: 1,
            split_role: "lead".to_owned(),
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
    fn cjk_reading_duration_counts_two_characters_as_one_word() {
        // 12 个汉字 ≈ 6 个英文词 ≈ 1800ms。
        let brief = "一二三四五六七八九十甲乙";
        assert_eq!(estimated_storyboard_duration_ms(brief), 1_800);
    }

    #[test]
    fn mixed_reading_duration_sums_ascii_and_cjk_units() {
        // coffee / machine / 上海 = 3 个词元 ≈ 900ms。
        let brief = "coffee machine 上海";
        assert_eq!(estimated_storyboard_duration_ms(brief), 900);
    }

    #[test]
    fn full_script_rejects_too_few_shots_for_long_narration() {
        let brief = std::iter::repeat("word")
            .take(120)
            .collect::<Vec<_>>()
            .join(" ");
        // 120 词 ≈ 36s，至少 5 个镜头。content 只有 1 个镜头。
        assert!(validate_storyboard(&content("direct"), &[source()], &brief).is_err());
    }

    #[test]
    fn full_script_accepts_enough_shots_for_long_narration() {
        let brief = std::iter::repeat("word")
            .take(120)
            .collect::<Vec<_>>()
            .join(" ");
        let mut storyboard = content("direct");
        let mut sources = vec![source()];
        // 补足到 5 个镜头，每个 8 秒，总计 40s > 36s，且素材各不相同。
        for index in 2..=5 {
            let asset_id = format!("asset-{}", index);
            let mut extra_source = source();
            extra_source.asset_id = asset_id.clone();
            storyboard.shots.push(StoryboardShot {
                order_index: index,
                duration_ms: 8_000,
                purpose: "Additional shot".to_owned(),
                on_screen_text: String::new(),
                narration_text: String::new(),
                asset_id,
                source_start_ms: 0,
                source_end_ms: 8_000,
                reason: "Provides enough shots for the narration.".to_owned(),
                beat_id: "context".to_owned(),
                match_level: "contextual".to_owned(),
                beat_part_index: 1,
                beat_part_count: 1,
                split_role: "lead".to_owned(),
            });
            sources.push(extra_source);
        }
        storyboard.target_duration_ms = 40_000;
        assert!(validate_storyboard(&storyboard, &sources, &brief).is_ok());
    }

    #[test]
    fn beat_narration_longer_than_limit_is_rejected() {
        let mut storyboard = content("direct");
        // 40 个词 ≈ 12s，超过 8s 上限。
        storyboard.beats[0].narration = std::iter::repeat("word")
            .take(40)
            .collect::<Vec<_>>()
            .join(" ");
        assert!(validate_storyboard(&storyboard, &[source()], "brief").is_err());
    }

    #[test]
    fn normalize_keeps_a_valid_preferred_video_range() {
        let mut storyboard = content("direct");
        storyboard.shots[0].source_start_ms = 2_000;
        storyboard.shots[0].source_end_ms = 6_000;
        storyboard.shots[0].duration_ms = 4_000;
        let normalized = normalize_storyboard_candidate(storyboard, &[source()], "brief");
        assert_eq!(normalized.shots[0].source_start_ms, 2_000);
        assert_eq!(normalized.shots[0].source_end_ms, 6_000);
        assert_eq!(normalized.shots[0].duration_ms, 4_000);
    }

    #[test]
    fn normalize_splits_overlapping_ranges_on_the_same_asset() {
        let mut long = source();
        long.duration_ms = Some(20_000);
        let mut storyboard = content("direct");
        storyboard.beats.push(StoryboardBeat {
            id: "second".to_owned(),
            purpose: "Show another beat".to_owned(),
            required_visual: "A second verified view".to_owned(),
            narration: String::new(),
        });
        storyboard.uncovered_beat_ids.clear();
        storyboard.shots[0].source_start_ms = 0;
        storyboard.shots[0].source_end_ms = 20_000;
        storyboard.shots[0].duration_ms = 20_000;
        storyboard.shots.push(StoryboardShot {
            order_index: 2,
            duration_ms: 20_000,
            purpose: "Show another beat".to_owned(),
            on_screen_text: String::new(),
            narration_text: String::new(),
            asset_id: "asset-1".to_owned(),
            source_start_ms: 0,
            source_end_ms: 20_000,
            reason: "Same source reused.".to_owned(),
            beat_id: "second".to_owned(),
            match_level: "direct".to_owned(),
            beat_part_index: 1,
            beat_part_count: 1,
            split_role: "lead".to_owned(),
        });
        let normalized = normalize_storyboard_candidate(storyboard, &[long.clone()], "brief");
        assert!(validate_non_overlapping_after(&normalized, &[long]));
        assert!(
            normalized.shots[0].source_end_ms <= normalized.shots[1].source_start_ms
                || normalized.shots[1].source_end_ms <= normalized.shots[0].source_start_ms
        );
    }

    #[test]
    fn normalize_fills_missing_narration_from_the_beat() {
        let mut storyboard = content("direct");
        storyboard.shots[0].narration_text = String::new();
        let normalized = normalize_storyboard_candidate(storyboard, &[source()], "brief");
        assert_eq!(
            normalized.shots[0].narration_text,
            "This is the opening scene."
        );
        // 模型漏写字幕时，字幕应从口播首句兜底生成（非空且简短）。
        assert!(!normalized.shots[0].on_screen_text.is_empty());
        assert!(normalized.shots[0].on_screen_text.chars().count() <= 40);
    }

    #[test]
    fn normalize_fills_missing_subtitle_from_narration() {
        let mut storyboard = content("direct");
        storyboard.shots[0].on_screen_text = String::new();
        storyboard.shots[0].narration_text =
            "我们承诺持续生产稳定质量。任何降低成本的做法都会被复审。".to_owned();
        let normalized = normalize_storyboard_candidate(storyboard, &[source()], "brief");
        let subtitle = normalized.shots[0].on_screen_text.as_str();
        assert_eq!(subtitle, "我们承诺持续生产稳定质量");
        assert!(subtitle.chars().count() <= 40);
    }

    #[test]
    fn normalize_drops_uncovered_ids_that_already_have_shots() {
        let mut storyboard = content("direct");
        storyboard.uncovered_beat_ids = vec!["context".to_owned(), "missing".to_owned()];
        let normalized = normalize_storyboard_candidate(storyboard, &[source()], "brief");
        assert_eq!(normalized.uncovered_beat_ids, ["missing"]);
        assert!(validate_storyboard(&normalized, &[source()], "brief").is_ok());
    }

    fn validate_non_overlapping_after(
        content: &StoryboardContent,
        sources: &[StoryboardSource],
    ) -> bool {
        super::validate_non_overlapping_video_sources(&content.shots, sources).is_ok()
    }

    #[test]
    fn normalize_labels_split_shots_by_beat_group() {
        let mut storyboard = content("direct");
        // beat context 拆成三个连续镜头，最后一个故意不带 part 字段，
        // 验证 normalize 会按 beat 分组补全（1-based 连续编号 + lead/bridge/tail）。
        for index in 2..=3 {
            let mut split_shot = storyboard.shots[0].clone();
            split_shot.order_index = index;
            split_shot.beat_id = "context".to_owned();
            split_shot.asset_id = "asset-1".to_owned();
            split_shot.source_start_ms = 0;
            split_shot.source_end_ms = 10_000;
            if index == 3 {
                split_shot.beat_part_index = 0;
                split_shot.beat_part_count = 0;
                split_shot.split_role = String::new();
            }
            storyboard.shots.push(split_shot);
        }
        let normalized = normalize_storyboard_candidate(storyboard, &[source(), source()], "brief");
        let context_shots = normalized
            .shots
            .iter()
            .filter(|shot| shot.beat_id == "context")
            .collect::<Vec<_>>();
        assert_eq!(context_shots.len(), 3);
        let part_indices = context_shots
            .iter()
            .map(|shot| shot.beat_part_index)
            .collect::<Vec<_>>();
        assert_eq!(part_indices, vec![1, 2, 3]);
        assert!(context_shots
            .iter()
            .all(|shot| shot.beat_part_count == 3));
        assert_eq!(context_shots[0].split_role, "lead");
        assert_eq!(context_shots[1].split_role, "bridge");
        assert_eq!(context_shots[2].split_role, "tail");
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
    voice_id: Option<String>,
) -> Result<StoryboardVersion, String> {
    generate_storyboard_internal(app, project_id, editing_task_id, brief, voice_id.as_deref(), true)
}

/// Agent storyboard generation consumes only analysis evidence already ready
/// in the scoped project. Starting or reprioritizing analysis remains an
/// explicit `request_asset_analysis` tool decision.
pub(crate) fn generate_storyboard_for_agent(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    brief: String,
    voice_id: Option<String>,
) -> Result<StoryboardVersion, String> {
    generate_storyboard_internal(app, project_id, editing_task_id, brief, voice_id.as_deref(), false)
}

fn generate_storyboard_internal(
    app: AppHandle,
    project_id: String,
    editing_task_id: String,
    brief: String,
    voice_id: Option<&str>,
    schedule_visual_analysis: bool,
) -> Result<StoryboardVersion, String> {
    log::info!(
        "Starting AI storyboard generation. project_id={}, editing_task_id={}, brief_length={}, schedule_visual_analysis={}",
        project_id,
        editing_task_id,
        brief.len(),
        schedule_visual_analysis
    );
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
        log::info!("Prioritizing visual analysis batch for storyboard generation.");
        let priority_batch = prioritize_pending_visual_batches(&app, &project_id, brief)?;
        wait_for_visual_batch(&app, priority_batch.as_deref())?;
    }
    match crate::assets::analysis::backfill_project_visual_quality(&connection, &project_id) {
        Ok(updated) if updated > 0 => {
            log::info!("Backfilled visual quality for {updated} storyboard assets.");
        }
        Ok(_) => {}
        Err(_) => {
            log::warn!("Visual quality backfill unavailable; neutral quality remains active for affected assets.");
        }
    }

    match semantic::backfill_project_embeddings(&app, &connection, &project_id) {
        Ok(updated) if updated > 0 => {
            log::info!("Backfilled local semantic embeddings for {updated} storyboard assets.");
        }
        Ok(_) => {}
        Err(_) => {
            log::warn!("Local semantic embedding backfill unavailable; lexical storyboard ranking remains active.");
        }
    }
    let (sources, visual_ready_count) = storyboard_sources(&connection, &project_id)?;
    let usage_counts = storyboard_usage_counts(&connection, &project_id)?;
    let video_count = sources.iter().filter(|s| s.kind == "video").count();
    let image_count = sources.iter().filter(|s| s.kind == "image").count();
    let audio_count = sources.iter().filter(|s| s.kind == "audio").count();
    let other_count = sources.iter().filter(|s| s.kind == "other").count();

    log::info!(
        "Loaded storyboard sources: total_count={}, visual_ready_count={}, video_count={}, image_count={}, audio_count={}, other_count={}",
        sources.len(),
        visual_ready_count,
        video_count,
        image_count,
        audio_count,
        other_count
    );

    // 记录前 10 个素材的详细信息用于诊断
    if !sources.is_empty() {
        let sample: Vec<String> = sources
            .iter()
            .take(10)
            .map(|s| {
                format!(
                    "{}({}:{}ms)",
                    s.asset_id,
                    s.kind,
                    s.duration_ms.unwrap_or(0)
                )
            })
            .collect();
        log::info!("Sample of loaded sources (first 10): {}", sample.join(", "));
    }
    if sources.is_empty() {
        log::warn!(
            "No accessible source files found. visual_ready_count={}",
            visual_ready_count
        );
        return if visual_ready_count == 0 {
            Err("storyboard_visual_evidence_unavailable: visual_ready_candidates=0".to_owned())
        } else {
            Err(format!(
                "storyboard_source_inventory_unavailable: visual_ready_candidates={visual_ready_count}; accessible_source_files=0"
            ))
        };
    }
    let access = ModelAccess::resolve().map_err(|error| {
        log::warn!("AI storyboard generation could not access the configured provider: {error}.");
        error
    })?;

    // Phase 1: 生成叙事结构。若 beat 数或朗读时长不满足下限，带反馈重试。
    let mut phase1_feedback = None;
    let narrative = (0..MAX_PHASE1_REVISIONS).find_map(|revision| {
        log::info!("Phase 1 attempt {}/{}", revision + 1, MAX_PHASE1_REVISIONS);
        match phases::phase1_generate_narrative(&access, brief, phase1_feedback.as_deref()) {
            Ok(candidate) => {
                let estimated_duration = minimum_storyboard_duration(brief);
                let minimum_shot_count =
                    ((estimated_duration + 7_999) / 8_000).max(1) as usize;
                let beat_issue = (candidate.script_mode == "full_script"
                    && candidate.beats.len() < minimum_shot_count)
                    .then(|| {
                        format!(
                            "Storyboard should contain at least {} beats for the estimated {} ms of full-script narration; only {} beats were provided.",
                            minimum_shot_count, estimated_duration, candidate.beats.len()
                        )
                    });
                let narration_issue = candidate.beats.iter().find(|beat| {
                    let beat_duration = estimated_storyboard_duration_ms(&beat.narration);
                    beat_duration > MAX_BEAT_SPOKEN_MS
                }).map(|beat| {
                    format!(
                        "Beat '{}' narration reads for about {} ms, exceeding the {} ms per-beat limit; split it into shorter beats.",
                        beat.id,
                        estimated_storyboard_duration_ms(&beat.narration),
                        MAX_BEAT_SPOKEN_MS
                    )
                });
                let issue = beat_issue.or(narration_issue);
                if issue.is_none() {
                    Some(candidate)
                } else {
                    log::warn!("Phase 1 narrative rejected: {}", issue.clone().unwrap_or_default());
                    phase1_feedback = issue;
                    None
                }
            }
            Err(error) => {
                log::warn!("Phase 1 request failed: {error}");
                phase1_feedback = Some(error);
                None
            }
        }
    }).ok_or_else(|| {
        phase1_feedback
            .unwrap_or_else(|| "Storyboard narrative structure could not be generated.".to_owned())
    })?;
    // Audio-first: when full_script beats carry narration, pre-synthesize to obtain exact duration and override target duration so Phase 2/3 select shots around the true voiceover length. Non-critical: if TTS fails, keep estimated duration.
    let mut audio_first: Option<(i64, crate::voice_provider::AudioFirstPrepared)> = None;
    if narrative.script_mode == "full_script" {
        let narration_text = narrative
            .beats
            .iter()
            .map(|b| b.narration.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        if !narration_text.is_empty() {
            match crate::voice_provider::prepare_audio_first(&app, &project_id, &narration_text, voice_id) {
                Ok(prepared) => {
                    let hard_target = prepared.duration_ms.saturating_add(crate::timeline_voice::VOICEOVER_TAIL_MS);
                    log::info!("Audio-first prepared: duration={}ms hard_target={}ms reused={}", prepared.duration_ms, hard_target, prepared.reused_cache);
                    audio_first = Some((hard_target, prepared));
                }
                Err(e) => {
                    log::warn!("Audio-first prepare skipped (keeping estimate): {e}");
                }
            }
        }
    }
    let mut narrative = narrative;
    if let Some((hard_target, _)) = &audio_first {
        narrative.target_duration_ms = (*hard_target).clamp(3_000, 120_000);
    }
    log::info!(
        "Phase 1 complete: narrative with {} beats, target_duration={}ms audio_first={}",
        narrative.beats.len(),
        narrative.target_duration_ms,
        audio_first.is_some()
    );

    // Phase 2: 逐 beat 粗选镜
    let rough = phases::phase2_rough_shot_selection(
        &app,
        &access,
        brief,
        &narrative,
        &sources,
        &usage_counts,
    )?;
    log::info!(
        "Phase 2 complete: rough storyboard with {} shots, {} uncovered beats",
        rough.shots.len(),
        rough.uncovered_beat_ids.len()
    );

    // Phase 3: 精剪与验证重试循环
    //
    // 流程：模型生成候选 → Rust 收集结构性问题 → 语义问题打包成 RepairPacket
    // 回传模型继续决策 → Rust 做最后机械兜底（normalize + 只修正无歧义字段）。
    // Err 仅表示模型请求失败或 JSON 无法解析，直接重试；问题越修越少不重跑整条链路。
    // 每次回传携带：① 已确认正确的冻结镜头 ② 修复记忆（前几轮修了什么、结果如何）。
    let mut repair: Option<RepairPacket> = None;
    let mut content = None;
    for revision in 0..MAX_STORYBOARD_REVISIONS {
        log::info!(
            "Phase 3 attempt {}/{}: fine editing with validation",
            revision + 1,
            MAX_STORYBOARD_REVISIONS
        );
        match phases::phase3_fine_edit(&access, brief, &rough, &sources, repair.as_ref()) {
            Ok((candidate, issues)) => {
                log::info!(
                    "Phase 3 produced candidate: shots={}, beats={}, target_duration_ms={}, uncovered_beats={}, issues={}",
                    candidate.shots.len(),
                    candidate.beats.len(),
                    candidate.target_duration_ms,
                    candidate.uncovered_beat_ids.len(),
                    issues.len()
                );
                if issues.is_empty()
                    || !issues
                        .iter()
                        .any(|issue| issue.needs_model_decision)
                {
                    // 无问题，或只剩 Rust 可机械兜底的问题：接受候选，做最后机械修正。
                    let candidate = normalize_storyboard_candidate(candidate, &sources, brief);
                    match validate_storyboard(&candidate, &sources, brief) {
                        Ok(()) => {
                            log::info!("Storyboard validation passed.");
                            content = Some(candidate);
                            break;
                        }
                        Err(error) => {
                            log::warn!("Phase 3 validation failed: {error}");
                            repair = Some(RepairPacket::new(
                                revision + 1,
                                vec![StoryboardIssue::new(
                                    "validation",
                                    error,
                                    true,
                                )],
                            ));
                        }
                    }
                } else {
                    // 有语义问题：把结构化修复包回传给模型，让模型做下一步决策。
                    let semantic = issues
                        .iter()
                        .collect::<Vec<_>>();
                    for issue in &semantic {
                        log::warn!(
                            "Phase 3 semantic issue [{}]: {} (shots={:?})",
                            issue.kind,
                            issue.message,
                            issue.affected_shots
                        );
                    }
                    // 冻结：未被任何问题点名的镜头视为已确认正确，模型应保持不动。
                    let frozen = crate::storyboard::repair::frozen_shot_indices(
                        candidate.shots.iter().map(|shot| shot.order_index),
                        &issues,
                    );
                    let previous_shots = candidate
                        .shots
                        .iter()
                        .map(|shot| crate::storyboard::repair::ShotSnapshot {
                            shot_index: shot.order_index,
                            beat_id: shot.beat_id.clone(),
                            asset_id: shot.asset_id.clone(),
                            duration_ms: shot.duration_ms,
                            source_start_ms: shot.source_start_ms,
                            source_end_ms: shot.source_end_ms,
                        })
                        .collect::<Vec<_>>();
                    // 修复记忆：记录"这套修复指令本身用到的模型尝试历史"。
                    // 上一轮生成的候选经过本轮的校验，若某类问题不再出现，
                    // 说明模型上一轮修对了，记入记忆避免模型回退。
                    let repair_history = if let Some(previous) = repair.as_ref() {
                        let unresolved_kinds = issues
                            .iter()
                            .map(|issue| issue.kind.clone())
                            .collect::<std::collections::HashSet<_>>();
                        let mut history = previous
                            .repair_history
                            .iter()
                            .map(|record| {
                                let mut latest = record.clone();
                                if !unresolved_kinds.contains(&record.kind) {
                                    latest.resolved = true;
                                }
                                latest
                            })
                            .collect::<Vec<_>>();
                        history.extend(issues.iter().map(|issue| {
                            crate::storyboard::repair::RepairRecord::new(
                                revision + 1,
                                issue.kind.clone(),
                                issue.affected_shots.clone(),
                                false,
                            )
                        }));
                        history
                    } else {
                        issues
                            .iter()
                            .map(|issue| {
                                crate::storyboard::repair::RepairRecord::new(
                                    revision + 1,
                                    issue.kind.clone(),
                                    issue.affected_shots.clone(),
                                    false,
                                )
                            })
                            .collect()
                    };
                    repair = Some(RepairPacket::with_context(
                        revision + 1,
                        issues,
                        previous_shots,
                        frozen,
                        repair_history,
                    ));
                }
            }
            Err(error) => {
                log::warn!("Phase 3 request failed: {error}");
                repair = Some(RepairPacket::new(
                    revision + 1,
                    vec![StoryboardIssue::new(
                        "request_failed",
                        error,
                        false,
                    )],
                ));
            }
        }
    }
    let content = content.ok_or_else(|| {
        let needs_model = repair
            .as_ref()
            .map(RepairPacket::needs_model_decision)
            .unwrap_or(false);
        let unresolved = repair
            .as_ref()
            .and_then(|packet| packet.issues.first())
            .map(|issue| issue.message.clone())
            .unwrap_or_else(|| "Storyboard generation did not produce a valid result.".to_owned());
        log::error!(
            "Storyboard generation failed after {} Phase 3 attempts. needs_model_decision={}, unresolved_issue={}",
            MAX_STORYBOARD_REVISIONS,
            needs_model,
            unresolved
        );
        unresolved
    })?;
    log::info!("Storyboard content finalized. Persisting to database.");
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
    if let Some((_, prepared)) = audio_first {
        let _ = finalize_audio_first_timeline(&app, &connection, &version, &editing_task_id, prepared);
    }
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
                .map_err(|e| {
                    log::warn!("Storyboard content could not be deserialized: {e}");
                    rusqlite::Error::InvalidQuery
                })?;
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

fn finalize_audio_first_timeline(
    app: &AppHandle,
    connection: &Connection,
    storyboard: &StoryboardVersion,
    editing_task_id: &str,
    prepared: crate::voice_provider::AudioFirstPrepared,
) -> Result<(), String> {
    use crate::models::{TimelineClip, TimelineContent, TextTrack, TextCue, TextLayout, TextStyle, TextAnimation, VoiceoverTrack, VoiceoverCue};
    use crate::voice_provider::{cues_from_alignment, subtitle_track_from_cues};
    let duration_ms = prepared.duration_ms;
    let generation_id = prepared.cached.generation_id.clone();
    let voice_id = prepared.cached.manifest.voice_id.clone();
    let voice_name = prepared.cached.manifest.voice_name.clone();
    let mut cursor = 0_i64;
    let clips: Vec<TimelineClip> = storyboard
        .shots
        .iter()
        .map(|s| {
            let end = cursor + s.duration_ms;
            let clip = TimelineClip {
                shot_index: s.order_index,
                asset_id: s.asset_id.clone(),
                source_start_ms: s.source_start_ms,
                source_end_ms: s.source_end_ms,
                timeline_start_ms: cursor,
                timeline_end_ms: end,
                on_screen_text: s.on_screen_text.clone(),
                clip_kind: "source".to_owned(),
                derived_from_shot_index: None,
                fit_reason: None,
            };
            cursor = end;
            clip
        })
        .collect();
    let visual_ms = cursor;
    if visual_ms == 0 {
        return Ok(());
    }
    let voiceover_fits = visual_ms >= duration_ms;
    if !voiceover_fits {
        log::warn!(
            "Audio-first deficit: visual={visual_ms} voice={duration_ms}; creating picture-only timeline for insert_clips/change_clip_duration repair"
        );
    }
    // Build draft timeline for draft-then-promote pattern: we create timeline_versions entry ourselves to reuse insert path
    let mut cues: Vec<TextCue> = Vec::new();
    let mut tcursor = 0_i64;
    for shot in &storyboard.shots {
        let start = tcursor;
        let end = start + shot.duration_ms;
        tcursor = end;
        if shot.on_screen_text.trim().is_empty() { continue; }
        cues.push(TextCue { id: format!("shot-{}-subtitle", shot.order_index), template_id: Some("subtitle_safe".to_owned()), start_ms: start, end_ms: end, text: shot.on_screen_text.chars().take(280).collect(), style: TextStyle::default(), layout: TextLayout::default(), entrance: Some(TextAnimation{template_id:"fade".to_owned(),duration_ms:180,intensity:0.6}), exit: Some(TextAnimation{template_id:"fade".to_owned(),duration_ms:160,intensity:0.5}), loop_animation: None, jianying_compatibility:"verified".to_owned() });
    }
    let mut text_tracks: Vec<TextTrack> = if cues.is_empty() { Vec::new() } else {
        vec![TextTrack{id:"storyboard-subtitles".to_owned(), role:"subtitle".to_owned(), layer:1, enabled:true, origin:"storyboard_generated".to_owned(), generation_id: None, editable:true, locked:false, cues}]
    };
    let mut vt: Vec<VoiceoverTrack> = Vec::new();
    if voiceover_fits {
        if let Ok(align_cues) = cues_from_alignment(&prepared.alignment, duration_ms) {
            let generated = subtitle_track_from_cues(&generation_id, &align_cues);
            let kept: Vec<TextTrack> = text_tracks
                .into_iter()
                .filter(|t| {
                    !(t.role == "subtitle"
                        && !t.locked
                        && matches!(t.origin.as_str(), "storyboard_generated" | "voice_alignment"))
                })
                .collect();
            let mut new_tracks = kept;
            new_tracks.push(generated);
            text_tracks = new_tracks;
        }
        let mut voiceover = VoiceoverTrack {
            id: format!("voiceover-{generation_id}"),
            enabled: true,
            cues: vec![VoiceoverCue {
                id: format!("voiceover-{generation_id}-cue"),
                asset_id: String::new(),
                generation_id: generation_id.clone(),
                source_start_ms: 0,
                source_end_ms: duration_ms,
                timeline_start_ms: 0,
                timeline_end_ms: duration_ms,
                volume: 1.0,
                fade_in_ms: 0,
                fade_out_ms: 80,
                provider: "ElevenLabs".to_owned(),
                voice_id: voice_id.clone(),
                voice_name: voice_name.clone(),
            }],
        };
        match (|| -> Result<String, String> {
            let mp3_path = prepared.cached.directory.join("voiceover.mp3");
            let disp = format!("ElevenLabs: {} voiceover", voice_name);
            let asset =
                crate::assets::store_downloaded_audio(app, &storyboard.project_id, mp3_path, &disp)?;
            let asset = crate::assets::wait_for_asset_ready(app, &storyboard.project_id, &asset.id)?;
            Ok(asset.id)
        })() {
            Ok(asset_id) => {
                for cue in &mut voiceover.cues {
                    cue.asset_id = asset_id.clone();
                }
            }
            Err(error) => log::warn!("Audio-first asset persist skipped: {error}"),
        }
        vt = vec![voiceover];
    }
    // Use shared timeline helper to insert version; we synthesize content_json and status directly
    let version_number: i64 = connection.query_row("SELECT COALESCE(MAX(version_number),0)+1 FROM timeline_versions WHERE project_id=?1", params![storyboard.project_id], |r| r.get(0)).map_err(|e| e.to_string())?;
    let new_id = uuid::Uuid::new_v4().to_string();
    let created_at = crate::db::now_millis();
    let content = TimelineContent{ clips, text_tracks, music_tracks: Vec::new(), voiceover_tracks: vt, overlay_clips: Vec::new(), quality_report: None };
    let content_json = serde_json::to_string(&content).map_err(|e| e.to_string())?;
    connection.execute("INSERT INTO timeline_versions (id, project_id, storyboard_version_id, version_number, status, content_json, created_at) VALUES (?1,?2,?3,?4,'draft',?5,?6)", params![new_id, storyboard.project_id, storyboard.id, version_number, content_json, created_at]).map_err(|e| e.to_string())?;
    // Log operation
    let conversation_id: Option<String> = connection.query_row("SELECT id FROM conversations WHERE project_id=?1 AND editing_task_id=?2 ORDER BY updated_at DESC LIMIT 1", params![storyboard.project_id, editing_task_id], |r| r.get(0)).ok();
    let before = serde_json::Value::Null;
    let after: serde_json::Value = serde_json::from_str(&content_json).unwrap_or(serde_json::Value::Null);
    let _ = connection.execute("INSERT INTO operation_logs (id, project_id, editing_task_id, conversation_id, agent_task_id, actor, operation_type, entity_type, entity_id, before_json, after_json, created_at) VALUES (?1,?2,?3,?4,NULL,'agent','audio_first_timeline','timeline_version',?5,?6,?7,?8)", params![uuid::Uuid::new_v4().to_string(), storyboard.project_id, editing_task_id, conversation_id, new_id, serde_json::to_string(&before).unwrap_or_default(), serde_json::to_string(&after).unwrap_or_default(), created_at]);
    log::info!("Audio-first timeline v{} created voice_fits={} voice={}ms id={}", version_number, voiceover_fits, duration_ms, new_id);
    Ok(())
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
