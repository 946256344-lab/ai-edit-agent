// storyboard/phases.rs - 三阶段 storyboard 生成流程
//
// Phase 1: 叙事结构生成 - 模型根据 brief 拆分 beats，不涉及素材
// Phase 2: 逐 beat 粗选镜 - 对每个 beat 单独排序素材，提供专属 TOP-5 候选
// Phase 3: 精剪与节奏优化 - 调整时间范围、节奏控制、镜头组合和过渡

use crate::models::{StoryboardBeat, StoryboardContent, StoryboardShot, StoryboardSource};
use crate::provider::ModelAccess;
use crate::storyboard::{
    model_response_json_text, post_model_payload, scoring, STORYBOARD_TIMEOUT,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::Duration;

const PHASE2_BEAT_TIMEOUT: Duration = Duration::from_secs(60);
const PHASE2_TOP_CANDIDATES: usize = 5;

/// Phase 1 输出：纯叙事结构
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NarrativeStructure {
    pub title: String,
    pub summary: String,
    pub target_duration_ms: i64,
    pub script_mode: String,
    pub beats: Vec<StoryboardBeat>,
}

/// Phase 2 输出：粗略 storyboard（每个 beat 一个 shot）
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoughStoryboard {
    pub title: String,
    pub summary: String,
    pub target_duration_ms: i64,
    pub script_mode: String,
    pub beats: Vec<StoryboardBeat>,
    pub uncovered_beat_ids: Vec<String>,
    pub shots: Vec<StoryboardShot>,
}

/// Phase 1: 生成叙事结构
pub(crate) fn phase1_generate_narrative(
    access: &ModelAccess,
    brief: &str,
) -> Result<NarrativeStructure, String> {
    log::info!("Phase 1: Generating narrative structure from brief");

    let prompt = format!(
        "Analyze this brief and create a narrative structure: {brief}\n\
        Return a JSON with: title, summary, targetDurationMs (3-120 seconds), scriptMode (full_script or key_message), and beats.\n\
        Each beat must contain: id (unique short slug), purpose (one sentence), requiredVisual (specific visual requirement), narration (spoken voiceover for this beat).\n\
        If the brief already contains speakable copy, split it across beats without repeating. If the brief has no speakable copy, write a short spoken line in the user's language. narration is voiceover, never on-screen titles.\n\
        Determine the appropriate number of beats based on the content's natural rhythm, pacing requirements, and narrative complexity. \
        A simple message might need just 3-4 beats, while a story-driven piece could use 8-12 or more. \
        Let the content guide the structure—do not artificially limit or pad the beat count. Do not select any media yet — this stage is pure story structure.\n\
        targetDurationMs is your creative proposal for the final video duration. scriptMode determines whether every word must be narrated (full_script) or only key points (key_message)."
    );

    let request = serde_json::json!({
        "model": access.custom_config().map(|c| c.model.as_str()).unwrap_or("gpt-5.4"),
        "store": false,
        "stream": true,
        "input": [{
            "role": "user",
            "content": [{ "type": "input_text", "text": prompt }]
        }],
        "text": { "format": { "type": "json_object" } }
    });

    let body = post_model_payload(access, &request, Some(STORYBOARD_TIMEOUT))?;
    let text = model_response_json_text(access, &body)
        .ok_or_else(|| "Phase 1 response did not contain JSON.".to_owned())?;

    log::info!(
        "Phase 1 complete: received narrative structure, json_length={} bytes",
        text.len()
    );

    serde_json::from_str(&text)
        .map_err(|_| "Phase 1 JSON did not match NarrativeStructure schema.".to_owned())
}

/// Phase 2: 逐 beat 粗选镜。每个 beat 从全库取出 5 个匹配预选，读关键帧后再选 1 个。
pub(crate) fn phase2_rough_shot_selection(
    access: &ModelAccess,
    brief: &str,
    narrative: &NarrativeStructure,
    sources: &[StoryboardSource],
) -> Result<RoughStoryboard, String> {
    log::info!(
        "Phase 2: Rough shot selection for {} beats",
        narrative.beats.len()
    );

    let usage_counts = std::collections::HashMap::new();
    let target_each = if narrative.beats.is_empty() {
        narrative.target_duration_ms
    } else {
        narrative.target_duration_ms / narrative.beats.len() as i64
    };
    let mut shots = Vec::new();
    let mut uncovered_beat_ids = Vec::new();
    let mut prior_selections = Vec::new();

    for (index, beat) in narrative.beats.iter().enumerate() {
        let ranked = scoring::rank_segment_candidates(
            sources.to_vec(),
            beat,
            target_each,
            &prior_selections,
            &usage_counts,
        );
        let top5: Vec<StoryboardSource> = ranked
            .into_iter()
            .take(PHASE2_TOP_CANDIDATES)
            .map(|item| item.source)
            .collect();
        log::info!(
            "Beat '{}': top {}: {}",
            beat.id,
            top5.len(),
            top5.iter()
                .map(|candidate| format!("{}({})", candidate.asset_id, candidate.kind))
                .collect::<Vec<_>>()
                .join(", ")
        );
        match pick_shot_for_beat(access, brief, beat, index as i64 + 1, target_each, &top5) {
            Ok(Some(shot)) => {
                prior_selections.push(shot.asset_id.clone());
                shots.push(shot);
            }
            Ok(None) => uncovered_beat_ids.push(beat.id.clone()),
            Err(error) => {
                log::warn!(
                    "Phase 2 beat '{}' pick failed; leaving uncovered: {error}",
                    beat.id
                );
                uncovered_beat_ids.push(beat.id.clone());
            }
        }
    }
    if shots.is_empty() {
        return Err(
            "storyboard_phase2_empty: no beat received a valid shot from its top candidates."
                .to_owned(),
        );
    }

    log::info!(
        "Phase 2 complete: {} shots, {} uncovered beats",
        shots.len(),
        uncovered_beat_ids.len()
    );

    Ok(RoughStoryboard {
        title: narrative.title.clone(),
        summary: narrative.summary.clone(),
        target_duration_ms: narrative.target_duration_ms,
        script_mode: narrative.script_mode.clone(),
        beats: narrative.beats.clone(),
        uncovered_beat_ids,
        shots,
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LooseShot {
    #[serde(default)]
    asset_id: String,
    #[serde(default)]
    duration_ms: i64,
    #[serde(default)]
    source_start_ms: i64,
    #[serde(default)]
    source_end_ms: i64,
    #[serde(default)]
    on_screen_text: String,
    #[serde(default)]
    narration_text: String,
    #[serde(default)]
    reason: String,
    #[serde(default)]
    match_level: String,
}

fn pick_shot_for_beat(
    access: &ModelAccess,
    brief: &str,
    beat: &StoryboardBeat,
    order_index: i64,
    target_duration_ms: i64,
    top5: &[StoryboardSource],
) -> Result<Option<StoryboardShot>, String> {
    if top5.is_empty() {
        return Ok(None);
    }
    let cards: Vec<Value> = top5
        .iter()
        .enumerate()
        .map(|(index, source)| compact_candidate_card(index, source))
        .collect();
    let prompt = format!(
        "Brief: {brief}\n\
        Beat id: {}\nPurpose: {}\nRequired visual: {}\nTarget duration for this beat: {target_duration_ms} ms\n\
        Candidates (choose exactly one, or uncover):\n{}\n\n\
        Look at each candidate's keyframe grid when provided. Return JSON with either \
        {{\"uncovered\": true}} or {{\"uncovered\": false, \"shot\": {{...}}}}.\n\
        shot must contain: orderIndex, durationMs, purpose, onScreenText, narrationText, assetId, sourceStartMs, sourceEndMs, reason, beatId, matchLevel.\n\
        narrationText is spoken voiceover. If the beat already has narration, keep it. If the brief has no copy, write a short spoken line. Never copy onScreenText into narrationText.\n\
        Use ONLY these candidate assetIds. matchLevel is direct or contextual. Images use sourceStartMs=0 and sourceEndMs=0.",
        beat.id,
        beat.purpose,
        beat.required_visual,
        serde_json::to_string_pretty(&cards).unwrap_or_else(|_| "[]".to_owned())
    );
    let mut content = vec![json!({"type": "input_text", "text": prompt})];
    for (index, source) in top5.iter().enumerate() {
        if let Some(image) = candidate_grid_image(source) {
            content.push(image);
            content.push(json!({
                "type": "input_text",
                "text": format!("Keyframe grid for candidate {index}, assetId {}.", source.asset_id)
            }));
        }
    }
    let request = json!({
        "model": access.custom_config().map(|c| c.model.as_str()).unwrap_or("gpt-5.4"),
        "store": false,
        "stream": true,
        "input": [{
            "role": "user",
            "content": content
        }],
        "text": { "format": { "type": "json_object" } }
    });
    let body = post_model_payload(access, &request, Some(PHASE2_BEAT_TIMEOUT))?;
    let text = model_response_json_text(access, &body)
        .ok_or_else(|| "storyboard_phase2: beat response did not contain JSON.".to_owned())?;
    let Some(loose) = parse_beat_pick(&text)? else {
        return Ok(None);
    };
    let allowed: Vec<&str> = top5.iter().map(|source| source.asset_id.as_str()).collect();
    if !allowed.contains(&loose.asset_id.as_str()) {
        log::warn!(
            "Phase 2 beat '{}' picked an asset outside the top {} candidates; leaving uncovered.",
            beat.id,
            PHASE2_TOP_CANDIDATES
        );
        return Ok(None);
    }
    let source = top5
        .iter()
        .find(|candidate| candidate.asset_id == loose.asset_id)
        .expect("allowed asset exists in top5");
    Ok(Some(shot_from_loose(
        loose,
        beat,
        order_index,
        target_duration_ms,
        source,
    )))
}

fn parse_beat_pick(text: &str) -> Result<Option<LooseShot>, String> {
    let value: Value = serde_json::from_str(text)
        .map_err(|_| "storyboard_phase2: beat JSON was invalid.".to_owned())?;
    if value.get("uncovered").and_then(Value::as_bool) == Some(true) {
        return Ok(None);
    }
    let shot_value = value.get("shot").cloned().unwrap_or(value);
    let shot: LooseShot = serde_json::from_value(shot_value)
        .map_err(|_| "storyboard_phase2: beat JSON did not match the pick schema.".to_owned())?;
    if shot.asset_id.trim().is_empty() {
        return Ok(None);
    }
    Ok(Some(shot))
}

fn shot_from_loose(
    loose: LooseShot,
    beat: &StoryboardBeat,
    order_index: i64,
    target_duration_ms: i64,
    source: &StoryboardSource,
) -> StoryboardShot {
    let duration_ms = if loose.duration_ms > 0 {
        loose.duration_ms
    } else {
        target_duration_ms.max(1)
    };
    let (source_start_ms, source_end_ms) = if source.kind == "image" {
        (0, 0)
    } else {
        let start = loose.source_start_ms.max(0);
        let end = if loose.source_end_ms > start {
            loose.source_end_ms
        } else {
            start + duration_ms
        };
        (start, end)
    };
    let match_level = if matches!(loose.match_level.as_str(), "direct" | "contextual") {
        loose.match_level
    } else {
        "contextual".to_owned()
    };
    StoryboardShot {
        order_index,
        duration_ms,
        purpose: beat.purpose.clone(),
        on_screen_text: loose.on_screen_text,
        narration_text: first_spoken_narration(&[
            loose.narration_text.as_str(),
            beat.narration.as_str(),
            beat.purpose.as_str(),
        ]),
        asset_id: loose.asset_id,
        source_start_ms,
        source_end_ms,
        reason: if loose.reason.trim().is_empty() {
            "Selected from the beat's top matching candidates.".to_owned()
        } else {
            loose.reason
        },
        beat_id: beat.id.clone(),
        match_level,
    }
}

fn compact_candidate_card(index: usize, source: &StoryboardSource) -> Value {
    let visual_tags: Vec<String> = source
        .visual_evidence
        .iter()
        .flat_map(|evidence| {
            evidence
                .subjects
                .iter()
                .chain(&evidence.actions)
                .chain(&evidence.products)
                .cloned()
                .chain(evidence.scene.clone())
        })
        .take(12)
        .collect();
    json!({
        "candidateIndex": index,
        "assetId": source.asset_id,
        "kind": source.kind,
        "durationMs": source.duration_ms,
        "hasKeyframeGrid": source.keyframe_grid_path.is_some(),
        "sceneSegments": source.scene_segments.iter().take(8).map(|segment| {
            json!({"startMs": segment.start_ms, "endMs": segment.end_ms})
        }).collect::<Vec<_>>(),
        "visualTags": visual_tags
    })
}

fn first_spoken_narration(candidates: &[&str]) -> String {
    candidates
        .iter()
        .map(|value| value.trim())
        .find(|value| !value.is_empty())
        .unwrap_or("")
        .to_owned()
}

fn candidate_grid_image(source: &StoryboardSource) -> Option<Value> {
    let path = source.keyframe_grid_path.as_deref()?;
    let bytes = std::fs::read(path).ok()?;
    if bytes.is_empty() {
        return None;
    }
    Some(json!({
        "type": "input_image",
        "image_url": format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes))
    }))
}

#[cfg(test)]
mod tests {
    use super::parse_beat_pick;

    #[test]
    fn wrapped_shot_json_is_accepted() {
        let text = r#"{"uncovered":false,"shot":{"assetId":"asset-1","durationMs":3000,"sourceStartMs":0,"sourceEndMs":3000}}"#;
        let shot = parse_beat_pick(text).expect("parse").expect("shot");
        assert_eq!(shot.asset_id, "asset-1");
        assert_eq!(shot.duration_ms, 3000);
    }

    #[test]
    fn top_level_shot_json_is_accepted() {
        let text = r#"{"assetId":"asset-2","reason":"matches the line","narrationText":"They check how materials are managed."}"#;
        let shot = parse_beat_pick(text).expect("parse").expect("shot");
        assert_eq!(shot.asset_id, "asset-2");
        assert_eq!(shot.narration_text, "They check how materials are managed.");
    }

    #[test]
    fn uncovered_flag_skips_the_beat() {
        let text = r#"{"uncovered":true}"#;
        assert!(parse_beat_pick(text).expect("parse").is_none());
    }
}

/// Phase 3: 精剪与节奏优化
pub(crate) fn phase3_fine_edit(
    access: &ModelAccess,
    brief: &str,
    rough: &RoughStoryboard,
    sources: &[StoryboardSource],
    feedback: Option<&str>,
) -> Result<StoryboardContent, String> {
    log::info!(
        "Phase 3: Fine editing {} shots with validation feedback",
        rough.shots.len()
    );

    let source_map_json =
        serde_json::to_string(sources).map_err(|_| "Could not serialize source map.".to_owned())?;

    let feedback_context = feedback.map_or(String::new(), |fb| {
        format!("\n\nPrevious attempt failed validation: {fb}\nRevise the timing and shot structure to pass validation.")
    });

    let prompt = format!(
        "Brief: {brief}\n\
        Rough Storyboard: {}\n\
        Available Sources (with scene segments): {source_map_json}\n\
        {feedback_context}\n\n\
        Refine this rough storyboard into a final, executable version:\n\
        1. Adjust source time ranges to align with scene boundaries where possible\n\
        2. Ensure no overlapping time ranges from the same video asset\n\
        3. Optimize shot durations for pacing (total should match targetDurationMs)\n\
        4. Consider splitting or merging shots if it improves the narrative flow\n\
        5. Ensure visual transitions between consecutive shots are smooth\n\n\
        Return the complete final JSON with: title, summary, targetDurationMs, scriptMode, beats, uncoveredBeatIds, and shots.\n\
        Each shot must contain: orderIndex, durationMs, purpose, onScreenText, narrationText, assetId, sourceStartMs, sourceEndMs, reason, beatId, matchLevel.\n\
        Keep or refine narrationText as spoken voiceover. Never copy onScreenText into narrationText. If a shot has no narrationText, write one from its beat.\n\
        matchLevel must be 'direct' (evidence visibly supports the beat) or 'contextual' (honest scene-setting).\n\
        Do NOT add new assets — only refine timing and structure of the existing rough shots.\n\
        This is the FINAL pass before execution.",
        serde_json::to_string(&rough).unwrap_or_default()
    );

    let request = serde_json::json!({
        "model": access.custom_config().map(|c| c.model.as_str()).unwrap_or("gpt-5.4"),
        "store": false,
        "stream": true,
        "input": [{
            "role": "user",
            "content": [{ "type": "input_text", "text": prompt }]
        }],
        "text": { "format": { "type": "json_object" } }
    });

    let body = post_model_payload(access, &request, Some(STORYBOARD_TIMEOUT))?;
    let text = model_response_json_text(access, &body)
        .ok_or_else(|| "Phase 3 response did not contain JSON.".to_owned())?;

    log::info!(
        "Phase 3 complete: final storyboard ready, json_length={} bytes",
        text.len()
    );

    let mut final_content: StoryboardContent = serde_json::from_str(&text)
        .map_err(|_| "Phase 3 JSON did not match StoryboardContent schema.".to_owned())?;

    // 填充 brief 字段（model 不返回，由 Rust 补全）
    final_content.brief = brief.to_owned();

    Ok(final_content)
}
