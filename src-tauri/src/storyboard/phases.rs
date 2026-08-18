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
use serde::{Deserialize, Serialize};

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
        Each beat must contain: id (unique short slug), purpose (one sentence), requiredVisual (specific visual requirement).\n\
        Create 3-12 beats that cover the brief's narrative arc. Do not select any media yet — this stage is pure story structure.\n\
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

/// Phase 2: 逐 beat 粗选镜
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

    // 对每个 beat 单独排序素材，构建候选清单
    let usage_counts = std::collections::HashMap::new(); // TODO: 从 DB 读取使用历史

    let mut beat_candidates_text = String::new();
    for beat in &narrative.beats {
        let ranked = scoring::rank_segment_candidates(
            sources.to_vec(),
            beat,
            narrative.target_duration_ms / narrative.beats.len() as i64, // 均分时长作为初步估算
            &vec![],
            &usage_counts,
        );

        let top5: Vec<&StoryboardSource> = ranked.iter().take(5).map(|s| &s.source).collect();

        log::info!(
            "Beat '{}': ranked {} candidates, top 5: {}",
            beat.id,
            ranked.len(),
            top5.iter()
                .map(|c| format!("{}({})", c.asset_id, c.kind))
                .collect::<Vec<_>>()
                .join(", ")
        );

        beat_candidates_text.push_str(&format!(
            "\n\n## Beat: {}\nPurpose: {}\nRequired Visual: {}\nTop 5 Candidates:\n{}",
            beat.id,
            beat.purpose,
            beat.required_visual,
            serde_json::to_string_pretty(&top5)
                .unwrap_or_else(|_| "serialization error".to_owned())
        ));
    }

    let prompt = format!(
        "Brief: {brief}\n\
        Narrative Structure: {}\n\
        {beat_candidates_text}\n\n\
        For each beat above, select ONE asset from its top 5 candidates and specify a time range.\n\
        Return a JSON with: title, summary, targetDurationMs, scriptMode, beats, uncoveredBeatIds, and shots.\n\
        Each shot must contain: orderIndex, durationMs, purpose, onScreenText, assetId, sourceStartMs, sourceEndMs, reason, beatId, matchLevel.\n\
        matchLevel must be 'direct' (evidence visibly supports the beat) or 'contextual' (honest scene-setting).\n\
        If NO candidate can honestly support a beat, put its id in uncoveredBeatIds and skip creating a shot for it.\n\
        Use ONLY the supplied candidates for each beat. For video, source times must be within duration. For images, sourceStartMs and sourceEndMs must both be 0.\n\
        This is a ROUGH pass — precise timing will be refined in Phase 3.",
        serde_json::to_string(&narrative).unwrap_or_default()
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
        .ok_or_else(|| "Phase 2 response did not contain JSON.".to_owned())?;

    log::info!(
        "Phase 2 complete: received {} shots, json_length={} bytes",
        serde_json::from_str::<RoughStoryboard>(&text)
            .as_ref()
            .map(|s| s.shots.len())
            .unwrap_or(0),
        text.len()
    );

    serde_json::from_str(&text)
        .map_err(|_| "Phase 2 JSON did not match RoughStoryboard schema.".to_owned())
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
        Each shot must contain: orderIndex, durationMs, purpose, onScreenText, assetId, sourceStartMs, sourceEndMs, reason, beatId, matchLevel.\n\
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
