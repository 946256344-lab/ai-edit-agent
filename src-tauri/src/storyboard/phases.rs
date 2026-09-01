// storyboard/phases.rs - 三阶段 storyboard 生成流程
//
// Phase 1: 叙事结构生成 - 模型根据 brief 拆分 beats，不涉及素材
// Phase 2: 逐 beat 粗选镜 - 对每个 beat 单独排序素材，提供专属 TOP-12 候选
// Phase 3: 精剪与节奏优化 - 调整时间范围、节奏控制、镜头组合和过渡

use crate::models::{StoryboardBeat, StoryboardContent, StoryboardShot, StoryboardSource};
use crate::provider::ModelAccess;
use crate::storyboard::repair::{repair_packet_prompt_block, RepairPacket, StoryboardIssue};
use crate::storyboard::{
    model_response_json_text, post_model_payload, scoring, STORYBOARD_TIMEOUT,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tauri::AppHandle;

const PHASE2_BEAT_TIMEOUT: Duration = Duration::from_secs(60);
const PHASE2_TOP_CANDIDATES: usize = 12;

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
pub(crate) struct BeatCandidatePool {
    beat_id: String,
    beat_purpose: String,
    candidates: Vec<StoryboardSource>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoughStoryboard {
    pub title: String,
    pub summary: String,
    pub target_duration_ms: i64,
    pub script_mode: String,
    pub beats: Vec<StoryboardBeat>,
    pub uncovered_beat_ids: Vec<String>,
    pub shots: Vec<StoryboardShot>,
    /// 每个已覆盖 beat 的 Phase 2 预选候选池。仅用于内存中的 Phase 3 素材约束，
    /// 不序列化进 prompt（避免请求体过大）；模型只看到下面的精简候选卡片。
    #[serde(default, skip_serializing)]
    pub candidate_pools: Vec<BeatCandidatePool>,
}

/// Phase 1: 生成叙事结构
pub(crate) fn phase1_generate_narrative(
    access: &ModelAccess,
    brief: &str,
    feedback: Option<&str>,
) -> Result<NarrativeStructure, String> {
    log::info!("Phase 1: Generating narrative structure from brief");

    let feedback_context = feedback.map_or(String::new(), |value| {
        format!(
            "\n\nPrevious attempt was too coarse or under-covered the brief: {value}\nRevise the beat segmentation to be finer-grained and cover every distinct idea in the brief."
        )
    });

    let prompt = format!(
        "Analyze this brief and create a narrative structure: {brief}\n\
        Return a JSON with: title, summary, targetDurationMs (3-120 seconds), scriptMode (full_script or key_message), and beats.\n\
        Each beat must contain: id (unique short slug), purpose (one sentence), requiredVisual (specific visual requirement), narration (spoken voiceover for this beat).\n\
        Use beat segmentation to express separate information points, not broad paragraph chunks. One beat should usually cover one concrete idea, action, or emotional turn. If a beat contains more than two spoken clauses, split it further.\n\
        Keep beats short and specific: aim for 6-12 seconds of spoken narration per beat when the brief is verbose, and do not collapse unrelated ideas into one beat.\n\
        If the brief already contains speakable copy, split it across beats without repeating. If the brief has no speakable copy, write a short spoken line in the user's language. narration is voiceover, never on-screen titles.\n\
        Determine the appropriate number of beats based on the content's natural rhythm, pacing requirements, narrative complexity, and the number of distinct information points. \
        A simple message might still need 4-6 beats if it contains multiple ideas, while a story-driven piece could use 8-14 or more. \
        Let the content guide the structure—do not artificially limit or pad the beat count. Do not select any media yet — this stage is pure story structure.\n\
        targetDurationMs is your creative proposal for the final video duration. scriptMode determines whether every word must be narrated (full_script) or only key points (key_message).\n\
        {feedback_context}"
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

/// Phase 2: 逐 beat 粗选镜。每个 beat 从全库取出 12 个匹配预选，读关键帧后再选 1 个。
pub(crate) fn phase2_rough_shot_selection(
    app: &AppHandle,
    access: &ModelAccess,
    brief: &str,
    narrative: &NarrativeStructure,
    sources: &[StoryboardSource],
    usage_counts: &HashMap<String, i32>,
) -> Result<RoughStoryboard, String> {
    log::info!(
        "Phase 2: Rough shot selection for {} beats",
        narrative.beats.len()
    );

    let target_each = if narrative.beats.is_empty() {
        narrative.target_duration_ms
    } else {
        narrative.target_duration_ms / narrative.beats.len() as i64
    };
    let mut shots = Vec::new();
    let mut uncovered_beat_ids = Vec::new();
    let mut candidate_pools = Vec::new();
    let mut prior_selections = Vec::new();
    let mut semantic_fallback_logged = false;

    for beat in &narrative.beats {
        let beat_text = format!("{} {}", beat.required_visual, beat.purpose);
        let beat_embedding =
            match crate::storyboard::semantic::encode_beat_semantics(app, &beat_text) {
                Ok(embedding) => Some(embedding),
                Err(_) => {
                    if !semantic_fallback_logged {
                        log::warn!(
                            "Local semantic ranking unavailable; using lexical storyboard ranking."
                        );
                        semantic_fallback_logged = true;
                    }
                    None
                }
            };
        let top_candidates = top_candidates_for_beat(
            sources,
            beat,
            target_each,
            &prior_selections,
            usage_counts,
            beat_embedding.as_deref(),
        );
        log::info!(
            "Beat '{}': top {}: {}",
            beat.id,
            top_candidates.len(),
            top_candidates
                .iter()
                .map(|candidate| format!("{}({})", candidate.asset_id, candidate.kind))
                .collect::<Vec<_>>()
                .join(", ")
        );
        let order_index = shots.len() as i64 + 1;
        match pick_shot_for_beat(
            access,
            brief,
            beat,
            order_index,
            target_each,
            &top_candidates,
        ) {
            Ok(Some(shot)) => {
                prior_selections.push(shot.asset_id.clone());
                shots.push(shot);
                candidate_pools.push(BeatCandidatePool {
                    beat_id: beat.id.clone(),
                    beat_purpose: beat.purpose.clone(),
                    candidates: top_candidates,
                });
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
        candidate_pools,
    })
}

fn top_candidates_for_beat(
    sources: &[StoryboardSource],
    beat: &StoryboardBeat,
    target_duration_ms: i64,
    prior_selections: &[String],
    usage_counts: &HashMap<String, i32>,
    beat_embedding: Option<&[f32]>,
) -> Vec<StoryboardSource> {
    let ranked = scoring::rank_segment_candidates(
        candidates_within_diversity_limit(sources, prior_selections),
        beat,
        target_duration_ms,
        prior_selections,
        usage_counts,
        beat_embedding,
    );
    ranked
        .into_iter()
        .take(PHASE2_TOP_CANDIDATES)
        .map(|item| item.source)
        .collect()
}

fn candidates_within_diversity_limit(
    sources: &[StoryboardSource],
    prior_selections: &[String],
) -> Vec<StoryboardSource> {
    let max_asset_uses = super::max_asset_uses_for_shot_count(prior_selections.len() + 1);
    let last_asset = prior_selections.last();
    sources
        .iter()
        .filter(|source| {
            last_asset != Some(&source.asset_id)
                && prior_selections
                    .iter()
                    .filter(|asset_id| *asset_id == &source.asset_id)
                    .count()
                    < max_asset_uses
        })
        .cloned()
        .collect()
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
    top_candidates: &[StoryboardSource],
) -> Result<Option<StoryboardShot>, String> {
    if top_candidates.is_empty() {
        return Ok(None);
    }
    let cards: Vec<Value> = top_candidates
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
        Every candidate is a verified video. Use ONLY these candidate assetIds. matchLevel is direct or contextual. sourceStartMs and sourceEndMs must stay inside the candidate duration.",
        beat.id,
        beat.purpose,
        beat.required_visual,
        serde_json::to_string_pretty(&cards).unwrap_or_else(|_| "[]".to_owned())
    );
    let mut content = vec![json!({"type": "input_text", "text": prompt})];
    for (index, source) in top_candidates.iter().enumerate() {
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
    let allowed: Vec<&str> = top_candidates
        .iter()
        .map(|source| source.asset_id.as_str())
        .collect();
    if !allowed.contains(&loose.asset_id.as_str()) {
        log::warn!(
            "Phase 2 beat '{}' picked an asset outside the top {} candidates; leaving uncovered.",
            beat.id,
            PHASE2_TOP_CANDIDATES
        );
        return Ok(None);
    }
    let source = top_candidates
        .iter()
        .find(|candidate| candidate.asset_id == loose.asset_id)
        .expect("allowed asset exists in top candidates");
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
        // Phase 2 每个 beat 只产出一个主镜头；拆分由 Phase 3 决定。
        beat_part_index: 1,
        beat_part_count: 1,
        split_role: "lead".to_owned(),
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

/// Phase 3 的精简候选池：每个 beat 只展示主 shot 和至多 3 个备选的精简卡片，
/// 不再把完整 StoryboardSource（含全部 visual/ocr 证据）注入 prompt。
/// main_asset_ids 是 Phase 2 实际选中的素材（不一定排在候选池第一位）。
const PHASE3_MAX_ALTERNATES: usize = 3;

fn phase3_candidate_cards(
    pools: &[BeatCandidatePool],
    main_asset_ids: &HashMap<String, String>,
) -> Vec<Value> {
    pools
        .iter()
        .filter_map(|pool| {
            if pool.candidates.is_empty() {
                return None;
            }
            let main_position = main_asset_ids
                .get(&pool.beat_id)
                .and_then(|asset| {
                    pool.candidates
                        .iter()
                        .position(|candidate| candidate.asset_id == *asset)
                })
                .unwrap_or(0);
            let mut cards = pool
                .candidates
                .iter()
                .enumerate()
                .map(|(index, candidate)| compact_candidate_card(index, candidate))
                .collect::<Vec<_>>();
            let main_shot = cards.remove(main_position);
            cards.truncate(PHASE3_MAX_ALTERNATES);
            Some(json!({
                "beatId": pool.beat_id,
                "beatPurpose": pool.beat_purpose,
                "mainShot": main_shot,
                "alternates": cards
            }))
        })
        .collect()
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
    use super::{
        candidates_within_diversity_limit, collect_phase3_issues, parse_beat_pick, phase3_candidate_cards,
        BeatCandidatePool, RoughStoryboard, PHASE2_TOP_CANDIDATES, PHASE3_MAX_ALTERNATES,
    };
    use crate::models::{StoryboardBeat, StoryboardContent, StoryboardShot, StoryboardSource};
    use std::collections::HashMap;

    fn shot(asset_id: &str) -> StoryboardShot {
        StoryboardShot {
            order_index: 1,
            duration_ms: 1_000,
            purpose: "purpose".to_owned(),
            on_screen_text: String::new(),
            narration_text: "narration".to_owned(),
            asset_id: asset_id.to_owned(),
            source_start_ms: 0,
            source_end_ms: 1_000,
            reason: "reason".to_owned(),
            beat_id: "beat-1".to_owned(),
            match_level: "direct".to_owned(),
            beat_part_index: 1,
            beat_part_count: 1,
            split_role: "lead".to_owned(),
        }
    }

    fn beat() -> StoryboardBeat {
        StoryboardBeat {
            id: "beat-1".to_owned(),
            purpose: "purpose".to_owned(),
            required_visual: "vehicle".to_owned(),
            narration: "narration".to_owned(),
        }
    }

    fn source(asset_id: &str) -> StoryboardSource {
        StoryboardSource {
            asset_id: asset_id.to_owned(),
            kind: "video".to_owned(),
            duration_ms: Some(10_000),
            scene_segments: Vec::new(),
            ocr_evidence: Vec::new(),
            visual_evidence: Vec::new(),
            visual_quality_score: Some(0.5),
            evidence_embedding: None,
            keyframe_grid_path: None,
        }
    }

    fn candidate_pool(beat_id: &str, asset_ids: &[&str]) -> BeatCandidatePool {
        BeatCandidatePool {
            beat_id: beat_id.to_owned(),
            beat_purpose: "purpose".to_owned(),
            candidates: asset_ids
                .iter()
                .map(|asset_id| source(asset_id))
                .collect(),
        }
    }

    #[test]
    fn phase2_sends_twelve_candidates_to_the_model() {
        assert_eq!(PHASE2_TOP_CANDIDATES, 12);
    }

    #[test]
    fn phase2_excludes_an_asset_after_it_reaches_the_diversity_limit() {
        let sources = vec![source("repeated"), source("available")];
        let prior = vec![
            "repeated".to_owned(),
            "other".to_owned(),
            "repeated".to_owned(),
        ];

        let candidates = candidates_within_diversity_limit(&sources, &prior);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].asset_id, "available");
    }

    #[test]
    fn phase2_uses_the_actual_selected_shot_count_for_the_diversity_limit() {
        let sources = vec![source("repeated"), source("available")];
        let prior = vec!["repeated".to_owned(), "other".to_owned()];

        let candidates = candidates_within_diversity_limit(&sources, &prior);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].asset_id, "available");
    }

    #[test]
    fn phase2_never_offers_the_immediately_previous_asset() {
        let sources = vec![source("previous"), source("available")];
        let prior = vec![
            "first".to_owned(),
            "second".to_owned(),
            "third".to_owned(),
            "previous".to_owned(),
        ];

        let candidates = candidates_within_diversity_limit(&sources, &prior);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].asset_id, "available");
    }

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

    #[test]
    fn phase3_cannot_replace_the_asset_selected_for_a_beat() {
        let rough = RoughStoryboard {
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![shot("selected")],
            candidate_pools: Vec::new(),
        };
        let mut final_content = StoryboardContent {
            brief: String::new(),
            title: "changed".to_owned(),
            summary: "changed".to_owned(),
            target_duration_ms: 2_000,
            script_mode: "full_script".to_owned(),
            beats: vec![beat()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![shot("outside")],
        };

        assert!(!collect_phase3_issues(&mut final_content, &rough).is_empty());
    }

    #[test]
    fn collect_phase3_issues_reports_every_violation_not_just_the_first() {
        // 同时违反多个规则：首镜头换素材 + 第二个镜头重复用素材 + 用别的 beat 素材。
        // 修复包必须一次性收集全部问题，让模型在同一次修复中看到完整反馈。
        let mut beat_one = beat();
        beat_one.id = "beat-1".to_owned();
        let mut beat_two = beat();
        beat_two.id = "beat-2".to_owned();
        let mut rough_a = shot("selected-a");
        rough_a.beat_id = "beat-1".to_owned();
        let mut rough_b = shot("selected-b");
        rough_b.beat_id = "beat-2".to_owned();
        let rough = RoughStoryboard {
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat_one, beat_two],
            uncovered_beat_ids: Vec::new(),
            shots: vec![rough_a.clone(), rough_b],
            candidate_pools: vec![candidate_pool("beat-1", &["selected-a", "alt-a"])],
        };
        // beat-1 拆成两个 shot：首镜头换成别的 beat 的素材，第二个镜头又用同一个越界素材。
        let mut wrong_first = shot("selected-b");
        wrong_first.beat_id = "beat-1".to_owned();
        let mut wrong_second = shot("selected-b");
        wrong_second.beat_id = "beat-1".to_owned();
        let mut final_content = StoryboardContent {
            brief: String::new(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: Vec::new(),
            uncovered_beat_ids: Vec::new(),
            shots: vec![wrong_first, wrong_second],
        };

        let issues = collect_phase3_issues(&mut final_content, &rough);
        let kinds = issues
            .iter()
            .map(|issue| issue.kind.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert!(
            kinds.contains("first_shot_replaced"),
            "must report the first-shot replacement"
        );
        assert!(
            kinds.contains("outside_candidate_pool"),
            "must report the out-of-pool asset"
        );
        assert!(
            kinds.contains("duplicate_asset_in_beat"),
            "must report the duplicate asset"
        );
        // 全部是需要模型决策的语义问题，不能靠机械兜底静默接受。
        assert!(issues.iter().all(|issue| issue.needs_model_decision));
    }

    #[test]
    fn phase3_can_split_a_beat_into_distinct_candidate_shots() {
        let rough_shot = shot("selected");
        let rough = RoughStoryboard {
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![rough_shot.clone()],
            candidate_pools: vec![candidate_pool("beat-1", &["selected", "alt-a", "alt-b"])],
        };
        let mut alt_a = shot("alt-a");
        alt_a.beat_id = "beat-1".to_owned();
        let mut final_content = StoryboardContent {
            brief: String::new(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![rough_shot, alt_a],
        };

        let issues = collect_phase3_issues(&mut final_content, &rough);
        assert!(issues.is_empty(), "one beat may expand into shots from distinct pool candidates; issues={issues:?}");
        assert_eq!(final_content.shots.len(), 2);
        assert_eq!(final_content.uncovered_beat_ids, Vec::<String>::new());
    }

    #[test]
    fn phase3_cannot_reuse_the_same_candidate_within_a_beat() {
        let rough_shot = shot("selected");
        let rough = RoughStoryboard {
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![rough_shot.clone()],
            candidate_pools: vec![candidate_pool("beat-1", &["selected", "alt-a", "alt-b"])],
        };
        let mut final_content = StoryboardContent {
            brief: String::new(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![rough_shot.clone(), rough_shot],
        };

        assert!(!collect_phase3_issues(&mut final_content, &rough).is_empty());
    }

    #[test]
    fn phase3_cannot_reuse_another_beats_candidate() {
        let mut beat_one = beat();
        beat_one.id = "beat-1".to_owned();
        let mut beat_two = beat();
        beat_two.id = "beat-2".to_owned();
        let mut rough_a = shot("selected-a");
        rough_a.beat_id = "beat-1".to_owned();
        let mut rough_b = shot("selected-b");
        rough_b.beat_id = "beat-2".to_owned();
        let rough = RoughStoryboard {
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat_one, beat_two],
            uncovered_beat_ids: Vec::new(),
            shots: vec![rough_a.clone(), rough_b],
            // beat-1 的候选池是 selected-a；selected-b 只属于 beat-2。
            candidate_pools: vec![candidate_pool("beat-1", &["selected-a"])],
        };
        // beat-1 拆出第二个镜头，却用了 beat-2 的素材 selected-b：应被拒绝。
        let mut borrowed = shot("selected-b");
        borrowed.beat_id = "beat-1".to_owned();
        let mut final_content = StoryboardContent {
            brief: String::new(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: Vec::new(),
            uncovered_beat_ids: Vec::new(),
            shots: vec![rough_a, borrowed],
        };

        assert!(!collect_phase3_issues(&mut final_content, &rough).is_empty());
    }

    #[test]
    fn phase3_cannot_interleave_shots_from_another_beat() {
        let mut beat_one = beat();
        beat_one.id = "beat-1".to_owned();
        let mut beat_two = beat();
        beat_two.id = "beat-2".to_owned();
        let mut rough_a = shot("selected-a");
        rough_a.beat_id = "beat-1".to_owned();
        let mut rough_b = shot("selected-b");
        rough_b.beat_id = "beat-2".to_owned();
        let rough = RoughStoryboard {
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat_one, beat_two],
            uncovered_beat_ids: Vec::new(),
            shots: vec![rough_a, rough_b],
            candidate_pools: Vec::new(),
        };
        // beat-1, beat-2, beat-1 交错出现：不连续，应被拒绝。
        let mut interleaved = shot("selected-a");
        interleaved.beat_id = "beat-1".to_owned();
        let mut middle = shot("selected-b");
        middle.beat_id = "beat-2".to_owned();
        let mut final_content = StoryboardContent {
            brief: String::new(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: Vec::new(),
            uncovered_beat_ids: Vec::new(),
            shots: vec![interleaved.clone(), middle.clone(), interleaved],
        };

        assert!(!collect_phase3_issues(&mut final_content, &rough).is_empty());
    }

    #[test]
    fn phase3_split_shot_cannot_drop_the_first_beat_asset() {
        let mut beat_two = beat();
        beat_two.id = "beat-2".to_owned();
        let mut rough_a = shot("selected-a");
        rough_a.beat_id = "beat-1".to_owned();
        let mut rough_b = shot("selected-b");
        rough_b.beat_id = "beat-2".to_owned();
        let rough = RoughStoryboard {
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat(), beat_two],
            uncovered_beat_ids: Vec::new(),
            shots: vec![rough_a, rough_b],
            candidate_pools: Vec::new(),
        };
        // beat-1 的首镜头换成了 beat-2 的素材：应被拒绝。
        let mut first = shot("selected-b");
        first.beat_id = "beat-1".to_owned();
        let mut second = shot("selected-b");
        second.beat_id = "beat-2".to_owned();
        let mut final_content = StoryboardContent {
            brief: String::new(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: Vec::new(),
            uncovered_beat_ids: Vec::new(),
            shots: vec![first, second],
        };

        assert!(!collect_phase3_issues(&mut final_content, &rough).is_empty());
    }

    #[test]
    fn phase3_keeps_covered_shots_without_filling_uncovered_beats() {
        let mut uncovered_beat = beat();
        uncovered_beat.id = "beat-2".to_owned();
        let rough_shot = shot("selected");
        let rough = RoughStoryboard {
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat(), uncovered_beat],
            uncovered_beat_ids: vec!["beat-2".to_owned()],
            shots: vec![rough_shot.clone()],
            candidate_pools: Vec::new(),
        };
        let mut final_content = StoryboardContent {
            brief: String::new(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: Vec::new(),
            uncovered_beat_ids: Vec::new(),
            shots: vec![rough_shot],
        };

        let issues = collect_phase3_issues(&mut final_content, &rough);
        assert!(issues.is_empty(), "covered shot should remain valid; issues={issues:?}");
        assert_eq!(final_content.uncovered_beat_ids, vec!["beat-2"]);
    }

    #[test]
    fn phase3_cannot_add_a_shot_for_an_uncovered_beat() {
        let mut uncovered_beat = beat();
        uncovered_beat.id = "beat-2".to_owned();
        let mut added_shot = shot("selected");
        added_shot.beat_id = "beat-2".to_owned();
        let rough_shot = shot("selected");
        let rough = RoughStoryboard {
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat(), uncovered_beat],
            uncovered_beat_ids: vec!["beat-2".to_owned()],
            shots: vec![rough_shot.clone()],
            candidate_pools: Vec::new(),
        };
        let mut final_content = StoryboardContent {
            brief: String::new(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: Vec::new(),
            uncovered_beat_ids: Vec::new(),
            shots: vec![rough_shot, added_shot],
        };

        assert!(!collect_phase3_issues(&mut final_content, &rough).is_empty());
    }

    #[test]
    fn phase3_candidate_cards_expose_main_and_limited_alternates() {
        let pool = BeatCandidatePool {
            beat_id: "beat-1".to_owned(),
            beat_purpose: "purpose".to_owned(),
            candidates: vec![
                source("selected"),
                source("alt-a"),
                source("alt-b"),
                source("alt-c"),
                source("alt-d"),
            ],
        };
        let main_asset_ids = HashMap::from([("beat-1".to_owned(), "selected".to_owned())]);
        let cards = phase3_candidate_cards(&[pool], &main_asset_ids);
        assert_eq!(cards.len(), 1);
        let card = &cards[0];
        assert_eq!(card["beatId"], "beat-1");
        assert_eq!(card["mainShot"]["assetId"], "selected");
        let alternates = card["alternates"].as_array().expect("alternates array");
        assert_eq!(
            alternates.len(),
            PHASE3_MAX_ALTERNATES,
            "only a few alternates are injected"
        );
        let exposed_assets = alternates
            .iter()
            .map(|card| card["assetId"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(exposed_assets, vec!["alt-a", "alt-b", "alt-c"]);
        // 精简卡片不再携带完整证据数组。
        assert!(!card.to_string().contains("visualEvidence"));
        assert!(!card.to_string().contains("ocrEvidence"));
    }

    #[test]
    fn phase3_candidate_main_shot_follows_the_phase2_selection() {
        // Phase 2 选中的素材不是候选池第一位时，mainShot 必须指向它，
        // 否则模型会照 prompt 用 candidates[0]，被 enforce 当作替换主素材拒绝。
        let pool = BeatCandidatePool {
            beat_id: "beat-1".to_owned(),
            beat_purpose: "purpose".to_owned(),
            candidates: vec![
                source("rank-one"),
                source("chosen"),
                source("alt-b"),
            ],
        };
        let main_asset_ids = HashMap::from([("beat-1".to_owned(), "chosen".to_owned())]);
        let cards = phase3_candidate_cards(&[pool], &main_asset_ids);
        assert_eq!(cards[0]["mainShot"]["assetId"], "chosen");
        let alternates = cards[0]["alternates"]
            .as_array()
            .expect("alternates array");
        let exposed_assets = alternates
            .iter()
            .map(|card| card["assetId"].as_str().unwrap_or_default())
            .collect::<Vec<_>>();
        assert_eq!(exposed_assets, vec!["rank-one", "alt-b"]);
        assert!(!exposed_assets.contains(&"chosen"));
    }

    #[test]
    fn phase3_candidate_cards_skip_empty_pools() {
        let cards = phase3_candidate_cards(&[], &HashMap::new());
        assert!(cards.is_empty());
    }
}

/// Phase 3: 精剪与节奏优化
///
/// 返回 `(候选, 校验问题)`：
/// - `Err` 仅表示模型请求失败或 JSON 无法解析（没有可修复的候选，应直接重试）；
/// - `Ok((content, issues))` 表示模型返回了可解析候选，`issues` 是本地校验发现
///   的结构化问题。语义问题由调用方打包成 `RepairPacket` 回传模型继续决策；
///   `phase3_fine_edit` 内部同时完成无歧义的机械标准化（子镜头字段等）。
pub(crate) fn phase3_fine_edit(
    access: &ModelAccess,
    brief: &str,
    rough: &RoughStoryboard,
    sources: &[StoryboardSource],
    repair: Option<&RepairPacket>,
) -> Result<(StoryboardContent, Vec<StoryboardIssue>), String> {
    log::info!(
        "Phase 3: Fine editing {} shots with validation feedback",
        rough.shots.len()
    );

    let selected_asset_ids = rough
        .shots
        .iter()
        .map(|shot| shot.asset_id.as_str())
        .collect::<HashSet<_>>();
    let selected_sources = sources
        .iter()
        .filter(|source| selected_asset_ids.contains(source.asset_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if selected_sources.len() != selected_asset_ids.len() {
        return Err("Phase 3 source scope was unavailable.".to_owned());
    }
    // 精简注入：只给模型精简卡片（assetId、场景段、视觉标签），
    // 不把完整 StoryboardSource（visual/ocr/全部证据）塞进 prompt。
    let source_cards_json = selected_sources
        .iter()
        .enumerate()
        .map(|(index, source)| compact_candidate_card(index, source))
        .collect::<Vec<_>>();
    let source_map_json = serde_json::to_string(&source_cards_json)
        .map_err(|_| "Could not serialize source map.".to_owned())?;
    // Phase 2 实际选中的主素材（不一定在候选池第一位）。
    let main_asset_ids = rough
        .shots
        .iter()
        .map(|shot| (shot.beat_id.clone(), shot.asset_id.clone()))
        .collect::<HashMap<_, _>>();
    let candidate_cards_json = serde_json::to_string(&phase3_candidate_cards(
        &rough.candidate_pools,
        &main_asset_ids,
    ))
    .unwrap_or_else(|_| "[]".to_owned());

    let feedback_context = repair.map_or(String::new(), repair_packet_prompt_block);

    let prompt = format!(
        "Brief: {brief}\n\
        Rough Storyboard: {}\n\
        Available Sources (with scene segments): {source_map_json}\n\
        Candidate cards per beat (mainShot + up to {} alternates): {candidate_cards_json}\n\
        {feedback_context}\n\n\
        Refine this rough storyboard into a final, executable version:\n\
        1. Adjust source time ranges to align with scene boundaries where possible\n\
        2. Ensure no overlapping time ranges from the same video asset\n\
        3. Optimize shot durations for pacing (total should match targetDurationMs)\n\
        4. Keep every covered beat in the same order. You MAY split one beat into 2-3 consecutive shots when that improves pacing or visual variety — all shots of the same beat must stay contiguous and in the same relative position. Never merge, drop, or reorder beats; never create shots for uncovered beats.\n\
        5. The first shot of each beat must keep its mainShot assetId. If you split a beat, every additional shot must use an assetId from that beat's candidate alternates — never an asset from another beat, and never the same asset twice within one beat. Each shot in a split needs a distinct candidate of the same beat.\n\
        6. When you split a beat, divide its narrationText across the shots so each shot carries a distinct portion of the spoken copy.\n\
        7. Ensure visual transitions between consecutive shots are smooth\n\n\
        Return the complete final JSON with: title, summary, targetDurationMs, scriptMode, beats, uncoveredBeatIds, and shots.\n\
        Each shot must contain: orderIndex, durationMs, purpose, onScreenText, narrationText, assetId, sourceStartMs, sourceEndMs, reason, beatId, matchLevel. When a beat is split into multiple shots, also set beatPartIndex (1-based position within the beat) and beatPartCount (total shots of the beat); a single-shot beat uses beatPartIndex 1 and beatPartCount 1.\n\
        Keep or refine narrationText as spoken voiceover. Never copy onScreenText into narrationText. If a shot has no narrationText, write one from its beat.\n\
        matchLevel must be 'direct' (evidence visibly supports the beat) or 'contextual' (honest scene-setting).\n\
        Do NOT add new assets — only refine timing and structure of the existing rough shots and their selected sources.\n\
        This is the FINAL pass before execution.",
        serde_json::to_string(&rough).unwrap_or_default(),
        PHASE3_MAX_ALTERNATES
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

    // 收集结构性问题（同时做无歧义的字段标准化），语义问题由调用方回传模型……
    let issues = collect_phase3_issues(&mut final_content, rough);
    // brief 及叙事结构由前两阶段拥有，模型只精调已选镜头的时间和节奏。
    final_content.brief = brief.to_owned();

    Ok((final_content, issues))
}

/// 校验 Phase 3 输出并收集**全部**结构性问题（不只第一个错误）。
///
/// 副作用：无条件标准化子镜头字段（beatPart*、splitRole），这是机械修正，
/// 即使存在语义问题也会执行，保证后续修复轮拿到已标准化的候选。
fn collect_phase3_issues(
    final_content: &mut StoryboardContent,
    rough: &RoughStoryboard,
) -> Vec<StoryboardIssue> {
    let mut issues = Vec::new();

    let selected_by_beat = rough
        .shots
        .iter()
        .map(|shot| (shot.beat_id.as_str(), shot.asset_id.as_str()))
        .collect::<HashMap<_, _>>();
    let selected_asset_ids = selected_by_beat
        .values()
        .copied()
        .collect::<std::collections::HashSet<_>>();
    // 每个 beat 的候选池：Phase 3 拆分镜头只能从这个池子里选素材；
    // 无候选池时（旧 rough 或测试）回退到 Phase 2 全部已选素材。
    let pools_by_beat = rough
        .candidate_pools
        .iter()
        .map(|pool| {
            let candidate_ids = pool
                .candidates
                .iter()
                .map(|candidate| candidate.asset_id.as_str())
                .collect::<std::collections::HashSet<_>>();
            (pool.beat_id.as_str(), candidate_ids)
        })
        .collect::<HashMap<_, _>>();
    let covered_beat_ids: Vec<&str> = rough
        .shots
        .iter()
        .map(|shot| shot.beat_id.as_str())
        .collect();

    if final_content.shots.is_empty() {
        issues.push(
            StoryboardIssue::new(
                "empty_shot_list",
                "Phase 3 produced an empty shot list.",
                true,
            )
            .allowing(vec![
                "pick one asset from each covered beat's candidate pool",
            ]),
        );
        return issues;
    }

    // 每个 covered beat 至少保留一个镜头，且镜头必须按 beat 顺序连续出现。
    // 一旦发现乱序，剩余所有 beat 的位置都不可信，只报一个结构化问题并终止，
    // 避免把同一个根因拆成多个重复问题。
    let mut beat_cursor = 0usize;
    for &beat_id in &covered_beat_ids {
        let group_start = beat_cursor;
        if beat_cursor >= final_content.shots.len()
            || final_content.shots[beat_cursor].beat_id.as_str() != beat_id
        {
            let beat_at_cursor = final_content
                .shots
                .get(beat_cursor)
                .map(|shot| format!("'{}'", shot.beat_id))
                .unwrap_or_else(|| "none (beat missing)".to_owned());
            issues.push(
                StoryboardIssue::new(
                    "beat_order_broken",
                    format!(
                        "Beat order is broken at beat '{}': next shot belongs to {beat_at_cursor}, but every covered beat must stay contiguous and in the same relative position as the rough storyboard.",
                        beat_id
                    ),
                    true,
                )
                .for_shots(
                    final_content
                        .shots
                        .iter()
                        .skip(beat_cursor)
                        .map(|shot| shot.order_index)
                        .collect(),
                )
                .allowing(vec![
                    "keep covered beats in the exact rough order",
                    "remove shots that were inserted at the wrong position",
                ]),
            );
            break;
        }
        let expected_asset = selected_by_beat
            .get(beat_id)
            .copied()
            .unwrap_or_default();
        let allowed_assets = match pools_by_beat.get(beat_id) {
            Some(pool) => pool.clone(),
            None => selected_asset_ids.clone(),
        };
        let mut used_in_beat = std::collections::HashSet::new();
        // 先数出该 beat 拆出的 shot 数，据此标准化子镜头字段。
        while beat_cursor < final_content.shots.len()
            && final_content.shots[beat_cursor].beat_id.as_str() == beat_id
        {
            beat_cursor += 1;
        }
        let group_count = beat_cursor - group_start;
        for (offset, shot) in final_content
            .shots
            .iter_mut()
            .enumerate()
            .take(beat_cursor)
            .skip(group_start)
        {
            if !allowed_assets.contains(shot.asset_id.as_str()) {
                issues.push(
                    StoryboardIssue::new(
                        "outside_candidate_pool",
                        format!(
                            "Shot {} for beat '{}' uses asset '{}', which is outside that beat's Phase 2 candidate pool.",
                            offset + 1,
                            beat_id,
                            shot.asset_id
                        ),
                        true,
                    )
                    .for_shots(vec![shot.order_index])
                    .allowing(vec![
                        "pick an assetId from the beat's candidate alternates only",
                        "or reduce this beat back to its main shot",
                    ]),
                );
            }
            if !used_in_beat.insert(shot.asset_id.clone()) {
                issues.push(
                    StoryboardIssue::new(
                        "duplicate_asset_in_beat",
                        format!(
                            "Shots of beat '{}' reuse asset '{}'; each split shot needs a distinct candidate of the same beat.",
                            beat_id, shot.asset_id
                        ),
                        true,
                    )
                    .for_shots(vec![shot.order_index])
                    .allowing(vec![
                        "replace the duplicated shot with another candidate from the same beat pool",
                        "or merge the duplicate back into the previous shot",
                    ]),
                );
            }
            // 无歧义的机械标准化：无论是否存在语义问题都补齐子镜头字段。
            let part_index = (offset - group_start + 1) as i64;
            shot.beat_part_index = part_index;
            shot.beat_part_count = group_count as i64;
            shot.split_role = if group_count <= 1 {
                "lead".to_owned()
            } else if part_index == 1 {
                "lead".to_owned()
            } else if part_index == group_count as i64 {
                "tail".to_owned()
            } else {
                "bridge".to_owned()
            };
        }
        // 该 beat 的第一个镜头必须保留 Phase 2 选择的素材。
        let first_shot = final_content
            .shots
            .iter()
            .find(|shot| shot.beat_id == beat_id);
        if let Some(first_shot) = first_shot {
            if first_shot.asset_id != expected_asset {
                issues.push(
                    StoryboardIssue::new(
                        "first_shot_replaced",
                        format!(
                            "The first shot of beat '{}' must keep its Phase 2 main asset '{}', but got '{}'.",
                            beat_id, expected_asset, first_shot.asset_id
                        ),
                        true,
                    )
                    .for_shots(vec![first_shot.order_index])
                    .allowing(vec![
                        "restore the beat's main asset as the first shot",
                        "then pick alternates only for the additional split shots",
                    ]),
                );
            }
        }
    }
    if beat_cursor != final_content.shots.len() {
        let extra_shots = final_content
            .shots
            .iter()
            .skip(beat_cursor)
            .map(|shot| shot.order_index)
            .collect::<Vec<_>>();
        issues.push(
            StoryboardIssue::new(
                "shots_outside_covered_beats",
                "Phase 3 added shots for uncovered or reordered beats.",
                true,
            )
            .for_shots(extra_shots)
            .allowing(vec![
                "remove any shot that is not part of a covered beat",
            ]),
        );
    }

    final_content.title = rough.title.clone();
    final_content.summary = rough.summary.clone();
    final_content.target_duration_ms = rough.target_duration_ms;
    final_content.script_mode = rough.script_mode.clone();
    final_content.beats = rough.beats.clone();
    final_content.uncovered_beat_ids = rough.uncovered_beat_ids.clone();
    issues
}
