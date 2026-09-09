// storyboard/phases.rs - Storyboard 分步生成
//
// Phase 1: 叙事结构
// Phase 2: 本地 Top-12 短名单（去同/去相似 + 补位）
// Phase 3: 从池中选出 2–3 个互异 asset（顺序）
// Phase 4: 选内容窗 → 段内精修切点（禁止换片）
// Phase 5: 由调用方执行 normalize + validate_storyboard

use crate::models::{StoryboardBeat, StoryboardContent, StoryboardShot, StoryboardSource};
use crate::provider::ModelAccess;
use crate::storyboard::repair::{repair_packet_prompt_block, RepairPacket, StoryboardIssue};
use crate::storyboard::semantic::{cosine_similarity, ocr_is_meaningful};
use crate::storyboard::{
    model_response_json_text, post_model_payload, scoring, STORYBOARD_TIMEOUT,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use tauri::AppHandle;

const PHASE2_TOP_CANDIDATES: usize = 12;
/// 证据向量余弦相似度达到该阈值视为「相似素材」，入池时互斥。
const PHASE2_SIMILARITY_COSINE: f64 = 0.92;
/// 视觉/OCR 标签 Jaccard 重叠达到该阈值视为相似。
const PHASE2_SIMILARITY_TAG_JACCARD: f64 = 0.55;

/// Phase 1 输出：纯叙事结构
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NarrativeStructure {
    pub title: String,
    pub summary: String,
    pub target_duration_ms: i64,
    pub script_mode: String,
    /// 完整口播原文（仅 full_script）。模型从 brief 抽出应照念的文案，不得改写。
    #[serde(default)]
    pub spoken_script: String,
    pub beats: Vec<StoryboardBeat>,
}

/// Phase 2 输出：粗略 storyboard（每个 beat 一个 shot）
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BeatCandidatePool {
    beat_id: String,
    beat_purpose: String,
    candidates: Vec<StoryboardSource>,
    /// 与 candidates 一一对应的本地召回分数；旧 pools_json 缺省为空。
    #[serde(default)]
    scores: Vec<scoring::CandidateScore>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoughStoryboard {
    #[serde(default)]
    pub(crate) speech_timing: super::timing::SpeechTiming,
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

/// Phase 1: 生成叙事结构（scriptMode 由调用方在请求前锁定）。
pub(crate) fn phase1_generate_narrative(
    access: &ModelAccess,
    brief: &str,
    required_script_mode: &str,
    feedback: Option<&str>,
) -> Result<NarrativeStructure, String> {
    log::info!(
        "Phase 1: Generating narrative structure from brief (required_script_mode={required_script_mode})"
    );

    let feedback_context = feedback.map_or(String::new(), |value| {
        format!(
            "\n\nPrevious attempt was too coarse or under-covered the brief: {value}\nRevise the beat segmentation to be finer-grained and cover every distinct idea in the brief."
        )
    });

    let mode_instructions = if required_script_mode == "full_script" {
        "REQUIRED scriptMode=full_script (locked by the system because the brief contains substantial speakable copy). Do not choose key_message.\n\
        Put the exact speakable script into spokenScript (strip only non-spoken instructions like 'please edit a video'; keep wording, order, and language unchanged — do not paraphrase, summarize, or invent). Split the SAME wording across beat.narration fields so concatenating beat narrations (with spaces) reproduces spokenScript without extras or omissions. Set each beat.onScreenText to \"\" (subtitles come from voice alignment later).\n\
        targetDurationMs must match the real spokenScript length; never invent a longer essay than spokenScript. Aim for about 4-8 seconds of spoken narration per beat; if a beat contains more than two spoken clauses, split it further."
    } else {
        "REQUIRED scriptMode=key_message (locked by the system because the brief is a goal/outline/theme, not a spoken script). Do not choose full_script.\n\
        spokenScript=\"\", set every beat.narration to \"\", and write a short on-screen marker in beat.onScreenText in the user's language (one idea per beat, at most 24 visible characters, no emoji).\n\
        Punchy on-screen markers only (no voiceover). targetDurationMs typically 8-15 seconds (at most 15s) unless the user explicitly asks for a longer runtime. Prefer 2-5 beats. Never inflate into a 30-90s essay."
    };

    let prompt = format!(
        "Analyze this brief and create a narrative structure: {brief}\n\
        Return a JSON with: title, summary, targetDurationMs (3-120 seconds), scriptMode (must be \"{required_script_mode}\"), spokenScript (string), and beats.\n\
        Each beat must contain: id (unique short slug), purpose (one sentence), requiredVisual (specific visual requirement), visualKeywords (array of 4-8 concrete English nouns/verbs naming what should be visible on screen — no abstract words; asset tags are English), narration (string), onScreenText (string).\n\
        {mode_instructions}\n\
        Use beat segmentation to express separate information points, not broad paragraph chunks. One beat should usually cover one concrete idea, action, or emotional turn.\n\
        Determine the appropriate number of beats from distinct information points. Do not select any media yet — this stage is pure story structure.\n\
        targetDurationMs is your creative proposal for the final video duration and must stay consistent with spoken narration length (full_script) or readable marker pacing (key_message).\n\
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

/// Phase 2: 本地短名单——排序 + 去同/去相似 + 补位到 Top-12；不调用模型选镜。
pub(crate) fn phase2_rough_shot_selection(
    _app: &AppHandle,
    _access: &ModelAccess,
    _brief: &str,
    narrative: &NarrativeStructure,
    sources: &[StoryboardSource],
    usage_counts: &HashMap<String, i32>,
    embeddings: &[Vec<f32>],
    speech_timing: super::timing::SpeechTiming,
) -> Result<RoughStoryboard, String> {
    log::info!(
        "Phase 2: Shortlisting Top-{} candidates for {} beats (local dedupe + backfill)",
        PHASE2_TOP_CANDIDATES,
        narrative.beats.len()
    );

    let target_each = if narrative.beats.is_empty() {
        narrative.target_duration_ms
    } else {
        narrative.target_duration_ms / narrative.beats.len() as i64
    };
    let mut uncovered_beat_ids = Vec::new();
    let mut candidate_pools = Vec::new();
    for (beat_index, beat) in narrative.beats.iter().enumerate() {
        let beat_embedding = embeddings.get(beat_index);
        let target_each = speech_timing.duration(&beat.id).unwrap_or(target_each);
        let ranked = scoring::rank_segment_candidates(
            sources.to_vec(),
            beat,
            target_each,
            &[],
            usage_counts,
            beat_embedding.map(Vec::as_slice),
        );
        let (pool, scores, library_exhausted) =
            dedupe_and_backfill_pool(&ranked, PHASE2_TOP_CANDIDATES);
        log::info!(
            "Beat '{}': poolSize={}, libraryExhausted={}, sample={}",
            beat.id,
            pool.len(),
            library_exhausted,
            pool.iter()
                .zip(scores.iter())
                .take(8)
                .map(|(candidate, score)| {
                    format!(
                        "{}(sem={:.1}/lex={:.1}/q={:.1}/d={:.1}/f={:.1}=total={:.1}{})",
                        candidate.asset_id,
                        score.semantic,
                        score.lexical,
                        score.quality,
                        score.duration,
                        score.freshness,
                        score.total,
                        if score.has_evidence { "" } else { ",noEv" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        );
        if pool.len() < 2 {
            log::warn!(
                "Beat '{}': only {} distinct non-similar candidates after full-library backfill; leaving uncovered",
                beat.id,
                pool.len()
            );
            uncovered_beat_ids.push(beat.id.clone());
            continue;
        }
        candidate_pools.push(BeatCandidatePool {
            beat_id: beat.id.clone(),
            beat_purpose: beat.purpose.clone(),
            candidates: pool,
            scores,
        });
    }

    if candidate_pools.is_empty() {
        return Err(
            "storyboard_phase2_empty: no beat received at least two distinct non-similar candidates."
                .to_owned(),
        );
    }

    log::info!(
        "Phase 2 complete: {} covered beat pools, {} uncovered beats",
        candidate_pools.len(),
        uncovered_beat_ids.len()
    );

    // 为下游 Phase 3 提供每 beat 的临时主镜（池内第一条）；真正 2–3 选片仍由 Phase 3 完成。
    let mut shots = Vec::new();
    for (index, pool) in candidate_pools.iter().enumerate() {
        let Some(main) = pool.candidates.first() else {
            continue;
        };
        let beat = narrative.beats.iter().find(|beat| beat.id == pool.beat_id);
        let duration = speech_timing
            .duration(&pool.beat_id)
            .unwrap_or(target_each)
            .max(1);
        let source_end = main
            .duration_ms
            .unwrap_or(duration)
            .clamp(duration, main.duration_ms.unwrap_or(duration).max(duration));
        shots.push(StoryboardShot {
            crop_focus: None,
            order_index: (index as i64) + 1,
            duration_ms: duration.min(source_end),
            purpose: pool.beat_purpose.clone(),
            on_screen_text: String::new(),
            narration_text: beat.map(|b| b.narration.clone()).unwrap_or_default(),
            asset_id: main.asset_id.clone(),
            source_start_ms: 0,
            source_end_ms: duration.min(source_end).max(1),
            reason: "Phase 2 shortlist lead candidate.".to_owned(),
            beat_id: pool.beat_id.clone(),
            match_level: "contextual".to_owned(),
            beat_part_index: 1,
            beat_part_count: 1,
            split_role: "lead".to_owned(),
        });
    }

    Ok(RoughStoryboard {
        speech_timing,
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

/// 按排名去同/去相似并补位到 `target_len`；返回 (池, 分数, 是否库已耗尽仍不足)。
fn dedupe_and_backfill_pool(
    ranked: &[scoring::ScoredCandidate],
    target_len: usize,
) -> (Vec<StoryboardSource>, Vec<scoring::CandidateScore>, bool) {
    let mut pool: Vec<StoryboardSource> = Vec::new();
    let mut scores: Vec<scoring::CandidateScore> = Vec::new();
    for candidate in ranked {
        if pool.len() >= target_len {
            break;
        }
        if pool
            .iter()
            .any(|existing| sources_are_similar(existing, &candidate.source))
        {
            continue;
        }
        pool.push(candidate.source.clone());
        scores.push(candidate.score.clone());
    }
    let library_exhausted = pool.len() < target_len;
    (pool, scores, library_exhausted)
}

fn sources_are_similar(left: &StoryboardSource, right: &StoryboardSource) -> bool {
    if left.asset_id == right.asset_id {
        return true;
    }
    if let (Some(a), Some(b)) = (
        left.evidence_embedding.as_deref(),
        right.evidence_embedding.as_deref(),
    ) {
        if cosine_similarity(a, b).is_some_and(|score| score >= PHASE2_SIMILARITY_COSINE) {
            return true;
        }
    }
    let left_tags = evidence_tag_set(left);
    let right_tags = evidence_tag_set(right);
    if !left_tags.is_empty() && !right_tags.is_empty() {
        let intersection = left_tags.intersection(&right_tags).count() as f64;
        let union = left_tags.union(&right_tags).count() as f64;
        if union > 0.0 && intersection / union >= PHASE2_SIMILARITY_TAG_JACCARD {
            return true;
        }
    }
    false
}

fn evidence_tag_set(source: &StoryboardSource) -> HashSet<String> {
    let mut tags = HashSet::new();
    for evidence in &source.visual_evidence {
        for value in evidence
            .subjects
            .iter()
            .chain(evidence.actions.iter())
            .chain(evidence.products.iter())
        {
            let normalized = value.trim().to_ascii_lowercase();
            if normalized.len() >= 2 {
                tags.insert(normalized);
            }
        }
        if let Some(scene) = &evidence.scene {
            let normalized = scene.trim().to_ascii_lowercase();
            if normalized.len() >= 2 {
                tags.insert(normalized);
            }
        }
    }
    for ocr in &source.ocr_evidence {
        if !ocr_is_meaningful(&ocr.text) {
            continue;
        }
        let normalized = ocr.text.trim().to_ascii_lowercase();
        if normalized.len() >= 2 {
            tags.insert(normalized);
        }
    }
    tags
}

#[cfg(test)]
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

fn compact_candidate_card(
    index: usize,
    source: &StoryboardSource,
    keyframe_grid_attached: bool,
    score: Option<&scoring::CandidateScore>,
) -> Value {
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
    let mut card = json!({
        "candidateIndex": index,
        "assetId": source.asset_id,
        "kind": source.kind,
        "durationMs": source.duration_ms,
        "hasKeyframeGrid": source.keyframe_grid_path.is_some(),
        "keyframeGridAttached": keyframe_grid_attached,
        "keyframeTimesMs": source.keyframes.iter().map(|frame| frame.time_ms).collect::<Vec<_>>(),
        "sceneSegments": source.scene_segments.iter().take(8).map(|segment| {
            json!({"startMs": segment.start_ms, "endMs": segment.end_ms})
        }).collect::<Vec<_>>(),
        "visualTags": visual_tags
    });
    if let Some(score) = score {
        card["retrievalScore"] = json!(score.retrieval_score_pct());
        card["matchedKeywords"] = json!(score.matched_keywords);
    }
    card
}

/// Phase 3 的精简候选池：每个 beat 只展示主 shot 和至多 3 个备选的精简卡片，
/// 不再把完整 StoryboardSource（含全部 visual/ocr 证据）注入 prompt。
/// main_asset_ids 是 Phase 2 实际选中的素材（不一定排在候选池第一位）。
/// Phase 3 仍可展示最多多少备选（测试与精简卡片截断用）。
const PHASE3_MAX_ALTERNATES: usize = 3;

/// 每个 beat 的完整短名单卡片（目标 Top-12），供 Phase 3 选片。
fn phase3_pool_cards(
    pools: &[BeatCandidatePool],
    beats: &[StoryboardBeat],
    attached_asset_ids: &HashSet<String>,
) -> Vec<Value> {
    pools
        .iter()
        .filter_map(|pool| {
            if pool.candidates.is_empty() {
                return None;
            }
            let beat = beats.iter().find(|beat| beat.id == pool.beat_id);
            let cards = pool
                .candidates
                .iter()
                .enumerate()
                .map(|(index, candidate)| {
                    compact_candidate_card(
                        index,
                        candidate,
                        attached_asset_ids.contains(&candidate.asset_id),
                        pool.scores.get(index),
                    )
                })
                .collect::<Vec<_>>();
            Some(json!({
                "beatId": pool.beat_id,
                "beatPurpose": pool.beat_purpose,
                "requiredVisual": beat.map(|item| item.required_visual.as_str()).unwrap_or(""),
                "visualKeywords": beat.map(|item| item.visual_keywords.clone()).unwrap_or_default(),
                "narration": beat.map(|item| item.narration.as_str()).unwrap_or(""),
                "onScreenText": beat.map(|item| item.on_screen_text.as_str()).unwrap_or(""),
                "candidates": cards
            }))
        })
        .collect()
}

/// 兼容旧测试：mainShot + 有限 alternates。
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
                .map(|(index, candidate)| {
                    compact_candidate_card(index, candidate, false, pool.scores.get(index))
                })
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

#[cfg(test)]
mod tests {
    use super::{
        apply_narration_phrase_duration_floor, candidates_within_diversity_limit,
        clamp_shots_to_chosen_windows, collect_phase3_issues, collect_phase4_issues,
        dedupe_and_backfill_pool, parse_beat_pick, phase3_candidate_cards, phase3_pool_cards,
        resolve_overlaps_within_chosen_windows, BeatCandidatePool, RoughStoryboard,
        PHASE2_TOP_CANDIDATES, PHASE3_MAX_ALTERNATES,
    };
    use crate::models::{StoryboardBeat, StoryboardContent, StoryboardShot, StoryboardSource};
    use std::collections::{HashMap, HashSet};

    #[test]
    fn dedupe_backfill_skips_duplicate_asset_ids_and_fills_to_target() {
        let ranked = (0..20)
            .map(|index| {
                let id = if index % 3 == 0 {
                    "dup".to_owned()
                } else {
                    format!("asset-{index}")
                };
                crate::storyboard::scoring::ScoredCandidate {
                    source: source(&id),
                    score: crate::storyboard::scoring::CandidateScore {
                        total: (20 - index) as f64,
                        semantic: 0.0,
                        lexical: 0.0,
                        quality: 0.0,
                        duration: 0.0,
                        freshness: 0.0,
                        has_evidence: true,
                        matched_keywords: vec![],
                    },
                }
            })
            .collect::<Vec<_>>();
        let (pool, scores, exhausted) = dedupe_and_backfill_pool(&ranked, 12);
        assert!(!exhausted);
        assert_eq!(pool.len(), 12);
        assert_eq!(scores.len(), 12);
        let unique = pool
            .iter()
            .map(|item| item.asset_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), 12);
        assert!(unique.contains("dup"));
    }

    fn shot(asset_id: &str) -> StoryboardShot {
        StoryboardShot {
            crop_focus: None,
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
            visual_keywords: vec![],
            narration: "narration".to_owned(),
            on_screen_text: String::new(),
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
            keyframes: Vec::new(),
            source_path: None,
        }
    }

    fn candidate_pool(beat_id: &str, asset_ids: &[&str]) -> BeatCandidatePool {
        BeatCandidatePool {
            beat_id: beat_id.to_owned(),
            beat_purpose: "purpose".to_owned(),
            candidates: asset_ids.iter().map(|asset_id| source(asset_id)).collect(),
            scores: vec![],
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
            speech_timing: Default::default(),
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
            speech_timing: Default::default(),
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
            speech_timing: Default::default(),
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
        assert!(
            issues.is_empty(),
            "one beat may expand into shots from distinct pool candidates; issues={issues:?}"
        );
        assert_eq!(final_content.shots.len(), 2);
        assert_eq!(final_content.uncovered_beat_ids, Vec::<String>::new());
    }

    #[test]
    fn phase3_requires_at_least_two_shots_when_pool_has_alternates() {
        let rough_shot = shot("selected");
        let rough = RoughStoryboard {
            speech_timing: Default::default(),
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
            shots: vec![rough_shot],
        };

        let issues = collect_phase3_issues(&mut final_content, &rough);
        assert!(
            issues
                .iter()
                .any(|issue| issue.kind == "beat_below_min_shots"),
            "single-shot covered beats with alternates must be rejected; issues={issues:?}"
        );
    }

    #[test]
    fn phase3_cannot_reuse_the_same_candidate_within_a_beat() {
        let rough_shot = shot("selected");
        let rough = RoughStoryboard {
            speech_timing: Default::default(),
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
    fn phase3_rejects_consecutive_same_asset_across_beats() {
        // 回归：跨 beat 衔接也不得相邻同片；否则 Phase 4 易切出交叠源窗。
        let mut beat_one = beat();
        beat_one.id = "beat-1".to_owned();
        let mut beat_two = beat();
        beat_two.id = "beat-2".to_owned();
        let mut shot_a = shot("alt-a");
        shot_a.order_index = 1;
        shot_a.beat_id = "beat-1".to_owned();
        let mut shot_b = shot("shared");
        shot_b.order_index = 2;
        shot_b.beat_id = "beat-1".to_owned();
        let mut shot_c = shot("shared");
        shot_c.order_index = 3;
        shot_c.beat_id = "beat-2".to_owned();
        let mut shot_d = shot("alt-b");
        shot_d.order_index = 4;
        shot_d.beat_id = "beat-2".to_owned();
        let rough = RoughStoryboard {
            speech_timing: Default::default(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 8_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat_one.clone(), beat_two.clone()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![shot_b.clone(), shot_c.clone()],
            candidate_pools: vec![
                candidate_pool("beat-1", &["shared", "alt-a", "alt-c"]),
                candidate_pool("beat-2", &["shared", "alt-b", "alt-d"]),
            ],
        };
        let mut final_content = StoryboardContent {
            brief: String::new(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 8_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat_one, beat_two],
            uncovered_beat_ids: Vec::new(),
            // beat-1 以 shared 收尾，beat-2 以 shared 开头 → 相邻同片。
            shots: vec![shot_a, shot_b, shot_c, shot_d],
        };
        let issues = collect_phase3_issues(&mut final_content, &rough);
        assert!(
            issues
                .iter()
                .any(|issue| issue.kind == "consecutive_duplicate_asset"),
            "adjacent same asset across beats must be rejected; issues={issues:?}"
        );
    }

    #[test]
    fn phase3_rejects_asset_over_diversity_limit_even_when_non_adjacent() {
        // 4 镜上限为 1：非相邻复用同一 asset 也必须在 Phase 3 拦下，避免 Phase 5→4 空转。
        let mut beat_one = beat();
        beat_one.id = "beat-1".to_owned();
        let mut beat_two = beat();
        beat_two.id = "beat-2".to_owned();
        let mut shot_a = shot("shared");
        shot_a.order_index = 1;
        shot_a.beat_id = "beat-1".to_owned();
        let mut shot_b = shot("alt-a");
        shot_b.order_index = 2;
        shot_b.beat_id = "beat-1".to_owned();
        let mut shot_c = shot("alt-b");
        shot_c.order_index = 3;
        shot_c.beat_id = "beat-2".to_owned();
        let mut shot_d = shot("shared");
        shot_d.order_index = 4;
        shot_d.beat_id = "beat-2".to_owned();
        let rough = RoughStoryboard {
            speech_timing: Default::default(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 8_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat_one.clone(), beat_two.clone()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![shot_a.clone(), shot_c.clone()],
            candidate_pools: vec![
                candidate_pool("beat-1", &["shared", "alt-a", "alt-c"]),
                candidate_pool("beat-2", &["shared", "alt-b", "alt-d"]),
            ],
        };
        let mut final_content = StoryboardContent {
            brief: String::new(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 8_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat_one, beat_two],
            uncovered_beat_ids: Vec::new(),
            shots: vec![shot_a, shot_b, shot_c, shot_d],
        };
        let issues = collect_phase3_issues(&mut final_content, &rough);
        assert!(
            issues
                .iter()
                .any(|issue| issue.kind == "asset_over_diversity_limit"),
            "non-adjacent reuse above 40% must be rejected; issues={issues:?}"
        );
        assert!(
            !issues
                .iter()
                .any(|issue| issue.kind == "consecutive_duplicate_asset"),
            "fixture must remain non-adjacent; issues={issues:?}"
        );
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
            speech_timing: Default::default(),
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
            speech_timing: Default::default(),
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
    fn phase3_may_lead_with_any_pool_candidate() {
        let rough_shot = shot("selected");
        let rough = RoughStoryboard {
            speech_timing: Default::default(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![rough_shot],
            candidate_pools: vec![candidate_pool("beat-1", &["selected", "alt-a", "alt-b"])],
        };
        let mut lead = shot("alt-a");
        lead.beat_id = "beat-1".to_owned();
        let mut follow = shot("alt-b");
        follow.beat_id = "beat-1".to_owned();
        let mut final_content = StoryboardContent {
            brief: String::new(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![lead, follow],
        };
        assert!(
            collect_phase3_issues(&mut final_content, &rough).is_empty(),
            "Phase 3 may order any distinct pool candidates"
        );
    }

    #[test]
    fn phase4_rejects_asset_swaps_from_selection() {
        let selected = StoryboardContent {
            brief: String::new(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![shot("selected"), {
                let mut alt = shot("alt-a");
                alt.beat_id = "beat-1".to_owned();
                alt
            }],
        };
        let mut refined = StoryboardContent {
            brief: selected.brief.clone(),
            title: selected.title.clone(),
            summary: selected.summary.clone(),
            target_duration_ms: selected.target_duration_ms,
            script_mode: selected.script_mode.clone(),
            beats: selected.beats.clone(),
            uncovered_beat_ids: selected.uncovered_beat_ids.clone(),
            shots: selected.shots.clone(),
        };
        refined.shots[1].asset_id = "intruder".to_owned();
        let issues = collect_phase4_issues(&mut refined, &selected);
        assert!(issues.iter().any(|issue| issue.kind == "asset_swapped"));
    }

    #[test]
    fn narration_duration_floor_extends_within_window() {
        use crate::storyboard::multimodal::Phase4ContentWindow;
        use std::collections::HashMap;

        let mut content = StoryboardContent {
            brief: String::new(),
            title: "t".to_owned(),
            summary: "s".to_owned(),
            target_duration_ms: 5_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![{
                let mut item = shot("selected");
                item.narration_text = "这是一句比较完整的口播文案需要足够画面".to_owned();
                item.source_start_ms = 1_000;
                item.source_end_ms = 1_500;
                item.duration_ms = 500;
                item
            }],
        };
        let window = Phase4ContentWindow {
            window_id: "selected:w0".to_owned(),
            asset_id: "selected".to_owned(),
            start_ms: 0,
            end_ms: 10_000,
        };
        let mut pick_map = HashMap::new();
        pick_map.insert(1, (window, false));
        apply_narration_phrase_duration_floor(&mut content, &pick_map);
        assert!(content.shots[0].duration_ms > 500);
        assert!(content.shots[0].source_end_ms <= 10_000);
    }

    #[test]
    fn clamp_shots_survives_when_start_already_at_window_end() {
        // 回归：start+1 > window.end 时旧 clamp 会 panic。
        use crate::storyboard::multimodal::Phase4ContentWindow;
        use std::collections::HashMap;

        let mut content = StoryboardContent {
            brief: String::new(),
            title: "t".to_owned(),
            summary: "s".to_owned(),
            target_duration_ms: 5_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![{
                let mut item = shot("selected");
                item.source_start_ms = 1_000;
                item.source_end_ms = 2_000;
                item.duration_ms = 1_000;
                item
            }],
        };
        let window = Phase4ContentWindow {
            window_id: "selected:w0".to_owned(),
            asset_id: "selected".to_owned(),
            start_ms: 0,
            end_ms: 1_000,
        };
        let mut pick_map = HashMap::new();
        pick_map.insert(1, (window, false));
        clamp_shots_to_chosen_windows(&mut content, &pick_map);
        assert!(content.shots[0].source_end_ms > content.shots[0].source_start_ms);
        assert!(content.shots[0].source_end_ms <= 1_000);
        assert!(content.shots[0].source_start_ms >= 0);
    }

    #[test]
    fn resolve_overlaps_packs_identical_cuts_inside_shared_window() {
        // 回归：Pass B 两镜同素材同切点时，窗内机械拆开，避免 Phase 5 因交叠整轮重跑 Phase 4。
        use crate::storyboard::multimodal::Phase4ContentWindow;

        let mut content = StoryboardContent {
            brief: String::new(),
            title: "t".to_owned(),
            summary: "s".to_owned(),
            target_duration_ms: 10_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![
                {
                    let mut item = shot("shared");
                    item.order_index = 1;
                    item.crop_focus = Some([0.4, 0.5]);
                    item.source_start_ms = 12_082;
                    item.source_end_ms = 16_915;
                    item.duration_ms = 4_833;
                    item
                },
                {
                    let mut item = shot("shared");
                    item.order_index = 2;
                    item.beat_id = "beat-2".to_owned();
                    item.crop_focus = Some([0.6, 0.5]);
                    item.source_start_ms = 12_082;
                    item.source_end_ms = 16_915;
                    item.duration_ms = 4_833;
                    item
                },
            ],
        };
        let window = Phase4ContentWindow {
            window_id: "shared:w0".to_owned(),
            asset_id: "shared".to_owned(),
            start_ms: 12_082,
            end_ms: 16_915,
        };
        let mut pick_map = HashMap::new();
        pick_map.insert(1, (window.clone(), false));
        pick_map.insert(2, (window, false));
        resolve_overlaps_within_chosen_windows(&mut content, &pick_map);
        assert!(
            content.shots[0].source_end_ms <= content.shots[1].source_start_ms
                || content.shots[1].source_end_ms <= content.shots[0].source_start_ms
        );
        assert!(content.shots[0].source_start_ms >= 12_082);
        assert!(content.shots[1].source_end_ms <= 16_915);
        assert!(content.shots.iter().all(|shot| shot.crop_focus.is_none()));
    }

    #[test]
    fn phase3_keeps_covered_shots_without_filling_uncovered_beats() {
        let mut uncovered_beat = beat();
        uncovered_beat.id = "beat-2".to_owned();
        let rough_shot = shot("selected");
        let rough = RoughStoryboard {
            speech_timing: Default::default(),
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
        assert!(
            issues.is_empty(),
            "covered shot should remain valid; issues={issues:?}"
        );
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
            speech_timing: Default::default(),
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
            scores: vec![],
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
            candidates: vec![source("rank-one"), source("chosen"), source("alt-b")],
            scores: vec![],
        };
        let main_asset_ids = HashMap::from([("beat-1".to_owned(), "chosen".to_owned())]);
        let cards = phase3_candidate_cards(&[pool], &main_asset_ids);
        assert_eq!(cards[0]["mainShot"]["assetId"], "chosen");
        let alternates = cards[0]["alternates"].as_array().expect("alternates array");
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

    #[test]
    fn phase3_pool_cards_expose_full_shortlist() {
        let pool = BeatCandidatePool {
            beat_id: "beat-1".to_owned(),
            beat_purpose: "purpose".to_owned(),
            candidates: (0..12).map(|i| source(&format!("a{i}"))).collect(),
            scores: vec![],
        };
        let cards = phase3_pool_cards(&[pool], &[], &HashSet::new());
        assert_eq!(cards.len(), 1);
        assert_eq!(
            cards[0]["candidates"].as_array().map(|items| items.len()),
            Some(12)
        );
        assert_eq!(cards[0]["candidates"][0]["keyframeGridAttached"], false);
        assert_eq!(cards[0]["requiredVisual"], "");
    }
}

/// Phase 3: 从每个 beat 的短名单中选出 2–3 个互异 asset（含顺序）。
///
/// 返回 `(候选, 校验问题)`：`Err` 仅表示传输/解析失败；语义问题进 `issues`。
pub(crate) fn phase3_select(
    access: &ModelAccess,
    brief: &str,
    rough: &RoughStoryboard,
    repair: Option<&RepairPacket>,
) -> Result<(StoryboardContent, Vec<StoryboardIssue>), String> {
    log::info!(
        "Phase 3: Selecting 2-3 assets per beat from {} pools (with keyframe grids)",
        rough.candidate_pools.len()
    );

    let (keyframe_blocks, attached_asset_ids) = phase3_keyframe_image_blocks(rough);
    let candidate_cards_json = serde_json::to_string(&phase3_pool_cards(
        &rough.candidate_pools,
        &rough.beats,
        &attached_asset_ids,
    ))
    .unwrap_or_else(|_| "[]".to_owned());
    let feedback_context = repair.map_or(String::new(), repair_packet_prompt_block);
    let covered_ids = covered_beat_ids(rough);
    let covered_n = covered_ids.len();
    let max_uses_if_two = super::max_asset_uses_for_shot_count(covered_n.saturating_mul(2));
    let max_uses_if_three = super::max_asset_uses_for_shot_count(covered_n.saturating_mul(3));
    let prompt = format!(
        "Brief: {brief}\n\
        Narrative title/summary/target: {} / {} / {}ms\n\
        Script mode: {}\n\
        Covered beat ids in order: {}\n\
        Uncovered beat ids (do not create shots for these): {}\n\
        Beat timing plan (milliseconds; Voice=TTS alignment or Pacing=marker readability; empty means not available): {}\n\
        Candidate pools (pick ONLY from each beat's candidates): {candidate_cards_json}\n\
        {feedback_context}\n\n\
        Keyframe grids are attached below for some candidate assetIds (2x2 overview). Candidates with keyframeGridAttached=false have no image in this request — judge them from visualTags only.\n\
        Each pool lists requiredVisual, visualKeywords, narration/onScreenText for that beat. Use those frames and tags to judge which assets best match each beat's purpose/requiredVisual/visualKeywords.\n\
        retrievalScore and matchedKeywords are a local shortlist hint only — final choice must follow visible evidence in the frames/tags, not the score alone.\n\
        Select the entire sequence together, including transitions across beat boundaries. Match the actual visual evidence first.\n\
        Prefer an establishing view followed by an informative detail, preserve complete actions, and keep subject/screen direction coherent. Avoid consecutive near-identical views; choose an opening that shows the subject and an ending that shows the result. Do not invent camera motion or events absent from the frames.\n\
        Only actually selected shots count as reuse. Resolve repetition across the final sequence, not across candidate pools.\n\
        Hard rule: adjacent shots in the final playback order must never share the same assetId (including across beat boundaries).\n\
        Hard rule: no single assetId may appear in more than 40% of the final shot list. With {covered_n} covered beats, that means at most {max_uses_if_two} uses if every beat has 2 shots, or at most {max_uses_if_three} uses if every beat has 3 shots. Prefer marking the weakest beat uncovered=true over violating this limit when the pools cannot supply enough distinct assets.\n\
        For EACH covered beat, choose 2 or 3 DISTINCT assetIds from that beat's candidates, in playback order.\n\
        You may mark a covered beat as uncovered=true only when none of its candidates honestly fit; then assetIds must be [].\n\
        Do NOT invent assetIds. Do NOT pick from another beat's pool. Do NOT refine source time ranges yet.\n\n\
        Return JSON only: {{\"selections\":[{{\"beatId\":\"...\",\"assetIds\":[\"a\",\"b\"],\"uncovered\":false}}]}}\n\
        Include exactly one selection object per covered beat id listed above.",
        rough.title,
        rough.summary,
        rough.target_duration_ms,
        rough.script_mode,
        covered_ids.join(", "),
        rough.uncovered_beat_ids.join(", "),
        serde_json::to_string(&rough.speech_timing).unwrap()
    );

    let mut content_blocks = vec![json!({ "type": "input_text", "text": prompt })];
    content_blocks.extend(keyframe_blocks);

    let request = serde_json::json!({
        "model": access.custom_config().map(|c| c.model.as_str()).unwrap_or("gpt-5.4"),
        "store": false,
        "stream": true,
        "input": [{
            "role": "user",
            "content": content_blocks
        }],
        "text": { "format": { "type": "json_object" } }
    });

    crate::storyboard::provider_trace::append_storyboard_trace(
        "Phase 3",
        None,
        repair.map(|packet| packet.attempt).unwrap_or(1),
        "request",
        &request,
    );
    let body = post_model_payload(access, &request, Some(STORYBOARD_TIMEOUT))?;
    let response_value =
        serde_json::from_str::<Value>(&body).unwrap_or_else(|_| json!({ "raw": body }));
    crate::storyboard::provider_trace::append_storyboard_trace(
        "Phase 3",
        None,
        repair.map(|packet| packet.attempt).unwrap_or(1),
        "response",
        &response_value,
    );
    let text = model_response_json_text(access, &body)
        .ok_or_else(|| "Phase 3 response did not contain JSON.".to_owned())?;

    let mut selected = assemble_phase3_selection(brief, rough, &text)?;
    let issues = collect_phase3_issues(&mut selected, rough);
    log::info!(
        "Phase 3 select complete: shots={}, uncovered={}, issues={}",
        selected.shots.len(),
        selected.uncovered_beat_ids.len(),
        issues.len()
    );
    for pool in &rough.candidate_pools {
        let chosen = selected
            .shots
            .iter()
            .filter(|shot| shot.beat_id == pool.beat_id)
            .map(|shot| shot.asset_id.as_str())
            .collect::<Vec<_>>();
        if chosen.is_empty() {
            log::info!("Phase 3 beat '{}': uncovered", pool.beat_id);
        } else {
            log::info!(
                "Phase 3 beat '{}': selected[{}]",
                pool.beat_id,
                chosen.join(",")
            );
        }
    }
    Ok((selected, issues))
}

/// 为 Phase 3 附上候选素材的导入期关键帧网格：按名次跨池轮询，避免后排 beat 饿死。
fn phase3_keyframe_image_blocks(rough: &RoughStoryboard) -> (Vec<Value>, HashSet<String>) {
    use crate::storyboard::multimodal::{read_input_image, PHASE3_MAX_GRID_IMAGES};
    use std::path::Path;

    let mut blocks = Vec::new();
    let mut attached = HashSet::new();
    let max_rank = rough
        .candidate_pools
        .iter()
        .map(|pool| pool.candidates.len())
        .max()
        .unwrap_or(0);
    for rank in 0..max_rank {
        for pool in &rough.candidate_pools {
            if blocks.len() / 2 >= PHASE3_MAX_GRID_IMAGES {
                log::info!(
                    "Phase 3 attached {} keyframe grid image(s) (round-robin, cap {})",
                    blocks.len() / 2,
                    PHASE3_MAX_GRID_IMAGES
                );
                return (blocks, attached);
            }
            let Some(candidate) = pool.candidates.get(rank) else {
                continue;
            };
            if attached.contains(&candidate.asset_id) {
                continue;
            }
            let Some(grid_path) = candidate.keyframe_grid_path.as_deref() else {
                continue;
            };
            let Some(image) = read_input_image(Path::new(grid_path)) else {
                continue;
            };
            attached.insert(candidate.asset_id.clone());
            blocks.push(json!({
                "type": "input_text",
                "text": format!("Keyframe grid (2x2) for assetId={}", candidate.asset_id)
            }));
            blocks.push(image);
        }
    }
    log::info!(
        "Phase 3 attached {} keyframe grid image(s) (round-robin)",
        blocks.len() / 2
    );
    (blocks, attached)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Phase3SelectResponse {
    #[serde(default)]
    selections: Vec<Phase3BeatSelection>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Phase3BeatSelection {
    beat_id: String,
    #[serde(default)]
    asset_ids: Vec<String>,
    #[serde(default)]
    uncovered: bool,
}

fn covered_beat_ids(rough: &RoughStoryboard) -> Vec<String> {
    if !rough.candidate_pools.is_empty() {
        rough
            .candidate_pools
            .iter()
            .map(|pool| pool.beat_id.clone())
            .collect()
    } else {
        rough
            .shots
            .iter()
            .map(|shot| shot.beat_id.clone())
            .collect()
    }
}

fn assemble_phase3_selection(
    brief: &str,
    rough: &RoughStoryboard,
    text: &str,
) -> Result<StoryboardContent, String> {
    let parsed: Phase3SelectResponse = serde_json::from_str(text)
        .map_err(|_| "Phase 3 JSON did not match selection schema.".to_owned())?;
    let by_beat = parsed
        .selections
        .into_iter()
        .map(|item| (item.beat_id.clone(), item))
        .collect::<HashMap<_, _>>();

    let mut uncovered = rough.uncovered_beat_ids.clone();
    let mut shots = Vec::new();
    let mut order_index = 1_i64;
    let per_beat_budget = if rough.beats.is_empty() {
        rough.target_duration_ms
    } else {
        rough.target_duration_ms / rough.beats.len().max(1) as i64
    };

    for beat_id in covered_beat_ids(rough) {
        let pool = rough
            .candidate_pools
            .iter()
            .find(|pool| pool.beat_id == beat_id);
        let Some(selection) = by_beat.get(&beat_id) else {
            return Err(format!(
                "Phase 3 selection missing covered beat '{beat_id}'."
            ));
        };
        if selection.uncovered || selection.asset_ids.is_empty() {
            if !uncovered.iter().any(|id| id == &beat_id) {
                uncovered.push(beat_id.clone());
            }
            continue;
        }
        let beat = rough.beats.iter().find(|beat| beat.id == beat_id);
        let per_beat_budget = rough
            .speech_timing
            .duration(&beat_id)
            .unwrap_or(per_beat_budget);
        let part_count = selection.asset_ids.len() as i64;
        for (part_offset, asset_id) in selection.asset_ids.iter().enumerate() {
            let source = pool
                .and_then(|pool| {
                    pool.candidates
                        .iter()
                        .find(|candidate| candidate.asset_id == *asset_id)
                })
                .or_else(|| {
                    // 无池时回退 rough lead（旧测试路径）
                    None
                });
            let duration = (per_beat_budget / part_count.max(1)).clamp(1_200, 6_000);
            let source_duration = source
                .and_then(|item| item.duration_ms)
                .unwrap_or(duration)
                .max(1);
            let source_end = duration.min(source_duration).max(1);
            let split_role = if part_count <= 1 {
                "lead"
            } else if part_offset == 0 {
                "lead"
            } else if part_offset + 1 == part_count as usize {
                "tail"
            } else {
                "bridge"
            };
            shots.push(StoryboardShot {
                crop_focus: None,
                order_index,
                duration_ms: source_end,
                purpose: beat
                    .map(|item| item.purpose.clone())
                    .unwrap_or_else(|| pool.map(|p| p.beat_purpose.clone()).unwrap_or_default()),
                on_screen_text: if part_offset == 0 {
                    beat.map(|item| {
                        if !item.on_screen_text.trim().is_empty() {
                            item.on_screen_text.clone()
                        } else {
                            String::new()
                        }
                    })
                    .unwrap_or_default()
                } else {
                    String::new()
                },
                narration_text: if part_offset == 0 {
                    beat.map(|item| item.narration.clone()).unwrap_or_default()
                } else {
                    String::new()
                },
                asset_id: asset_id.clone(),
                source_start_ms: 0,
                source_end_ms: source_end,
                reason: "Phase 3 selected asset; ranges pending Phase 4.".to_owned(),
                beat_id: beat_id.clone(),
                match_level: "contextual".to_owned(),
                beat_part_index: (part_offset as i64) + 1,
                beat_part_count: part_count,
                split_role: split_role.to_owned(),
            });
            order_index += 1;
        }
    }

    Ok(StoryboardContent {
        brief: brief.to_owned(),
        title: rough.title.clone(),
        summary: rough.summary.clone(),
        target_duration_ms: rough.target_duration_ms,
        script_mode: rough.script_mode.clone(),
        beats: rough.beats.clone(),
        uncovered_beat_ids: uncovered,
        shots,
    })
}

/// Phase 4: 先选内容窗 → 段内精修切点 → 旁白时长托底 → 不确定再局部加密。
/// 禁止更换 assetId。
pub(crate) fn phase4_refine_ranges(
    app: &AppHandle,
    access: &ModelAccess,
    brief: &str,
    selected: &StoryboardContent,
    rough: &RoughStoryboard,
    sources: &[StoryboardSource],
    repair: Option<&RepairPacket>,
) -> Result<(StoryboardContent, Vec<StoryboardIssue>), String> {
    use crate::storyboard::multimodal::{
        build_phase4_windows_from_keyframes, compose_timed_frame_grid, densify_times_in_range,
        extract_frames_at_times, read_input_image, Phase4ContentWindow,
        PHASE4_MAX_FRAME_SPACING_MS, PHASE4_REFINE_FRAMES, PHASE4_REFINE_SHOTS_PER_BATCH,
        PHASE4_UNCERTAIN_FRAMES,
    };
    use std::path::Path;

    log::info!(
        "Phase 4: window-select then in-window refine for {} locked shots (keyframe windows, no per-asset scene scan)",
        selected.shots.len()
    );

    let selected_asset_ids = selected
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
        return Err("Phase 4 source scope was unavailable.".to_owned());
    }

    let mut windows_by_asset: HashMap<String, Vec<Phase4ContentWindow>> = HashMap::new();
    let mut all_windows: Vec<Phase4ContentWindow> = Vec::new();
    for source in &selected_sources {
        let duration_ms = source.duration_ms.unwrap_or(0).max(1);
        let keyframe_times: Vec<i64> = source.keyframes.iter().map(|frame| frame.time_ms).collect();
        let windows =
            build_phase4_windows_from_keyframes(&source.asset_id, duration_ms, &keyframe_times);
        log::info!(
            "Phase 4 windows for {}: count={} from_keyframes={}",
            source.asset_id,
            windows.len(),
            keyframe_times.len()
        );
        windows_by_asset.insert(source.asset_id.clone(), windows.clone());
        all_windows.extend(windows);
    }
    if all_windows.is_empty() {
        return Err("Phase 4 could not build content windows.".to_owned());
    }

    let feedback_context = repair.map_or(String::new(), repair_packet_prompt_block);

    // —— Pass A：按素材贪心拆批，每批 ≤ PHASE4_PASS_A_MAX_IMAGES 张窗中点帧 ——
    use crate::storyboard::multimodal::PHASE4_PASS_A_MAX_IMAGES;
    let mut asset_order = selected_sources
        .iter()
        .map(|source| source.asset_id.clone())
        .collect::<Vec<_>>();
    asset_order.sort();
    let mut pass_a_batches: Vec<Vec<String>> = Vec::new();
    let mut current_batch: Vec<String> = Vec::new();
    let mut current_images = 0usize;
    for asset_id in &asset_order {
        let window_count = windows_by_asset
            .get(asset_id)
            .map(|windows| windows.len())
            .unwrap_or(0);
        if window_count == 0 {
            continue;
        }
        if !current_batch.is_empty() && current_images + window_count > PHASE4_PASS_A_MAX_IMAGES {
            pass_a_batches.push(std::mem::take(&mut current_batch));
            current_images = 0;
        }
        // 单素材窗数超过上限时仍独占一批（无法再拆）。
        current_batch.push(asset_id.clone());
        current_images += window_count;
    }
    if !current_batch.is_empty() {
        pass_a_batches.push(current_batch);
    }
    if pass_a_batches.is_empty() {
        return Err("Phase 4 pass A had no assets with content windows.".to_owned());
    }

    let mut all_picks: Vec<Phase4WindowPick> = Vec::new();
    let mut pass_a_frames = 0usize;
    for (batch_index, batch_assets) in pass_a_batches.iter().enumerate() {
        let batch_asset_set = batch_assets.iter().cloned().collect::<HashSet<_>>();
        let batch_windows = all_windows
            .iter()
            .filter(|window| batch_asset_set.contains(&window.asset_id))
            .cloned()
            .collect::<Vec<_>>();
        let batch_shots = selected
            .shots
            .iter()
            .filter(|shot| batch_asset_set.contains(&shot.asset_id))
            .map(|shot| {
                json!({
                    "orderIndex": shot.order_index,
                    "assetId": shot.asset_id,
                    "beatId": shot.beat_id,
                    "purpose": shot.purpose,
                    "narrationText": shot.narration_text,
                    "durationMs": shot.duration_ms,
                })
            })
            .collect::<Vec<_>>();
        let window_cards = serde_json::to_string(&batch_windows)
            .map_err(|_| "Could not serialize Phase 4 windows.".to_owned())?;
        let shot_cards = serde_json::to_string(&batch_shots)
            .map_err(|_| "Could not serialize Phase 4 shots.".to_owned())?;
        let mut pass_a_blocks = vec![json!({
            "type": "input_text",
            "text": format!(
                "Brief: {brief}\n\
                Pass A batch {}/{} — refine window picks ONLY for these assetIds: {:?}.\n\
                Locked shots for this batch (assetIds FINAL): {shot_cards}\n\
                Content windows for this batch: {window_cards}\n\
                {feedback_context}\n\n\
                Each attached image is the midpoint of one windowId (windows come from import keyframes / head-mid-tail thirds, not a fresh full-clip scene scan).\n\
                For EVERY locked shot in this batch, pick exactly one windowId that belongs to that shot's assetId.\n\
                Prefer actionable/on-brief content over setup/prelude/idle when both exist.\n\
                Set uncertain=true when the best window is ambiguous or you may cut mid spoken phrase.\n\
                Return JSON only: {{\"picks\":[{{\"orderIndex\":1,\"windowId\":\"asset:w0\",\"uncertain\":false}}]}}",
                batch_index + 1,
                pass_a_batches.len(),
                batch_assets
            )
        })];
        let mut batch_frame_count = 0usize;
        for window in &batch_windows {
            let Some(source) = selected_sources
                .iter()
                .find(|source| source.asset_id == window.asset_id)
            else {
                continue;
            };
            let Some(path) = source.source_path.as_deref() else {
                continue;
            };
            let frames = extract_frames_at_times(
                app,
                &window.asset_id,
                Path::new(path),
                &[window.mid_ms()],
                &format!(
                    "passA_b{}_{}",
                    batch_index + 1,
                    window.window_id.replace(':', "_")
                ),
            );
            for (time_ms, frame_path) in frames {
                let Some(image) = read_input_image(&frame_path) else {
                    continue;
                };
                pass_a_blocks.push(json!({
                    "type": "input_text",
                    "text": format!(
                        "windowId={} assetId={} [{},{}] midTimeMs={}",
                        window.window_id, window.asset_id, window.start_ms, window.end_ms, time_ms
                    )
                }));
                pass_a_blocks.push(image);
                batch_frame_count += 1;
            }
        }
        if batch_frame_count == 0 {
            return Err(format!(
                "Phase 4a batch {} had no readable window midpoint frames.",
                batch_index + 1
            ));
        }
        pass_a_frames += batch_frame_count;
        log::info!(
            "Phase 4 pass A batch {}/{}: {} asset(s), {} midpoint frame(s)",
            batch_index + 1,
            pass_a_batches.len(),
            batch_assets.len(),
            batch_frame_count
        );

        let pass_a_request = json!({
            "model": access.custom_config().map(|c| c.model.as_str()).unwrap_or("gpt-5.4"),
            "store": false,
            "stream": true,
            "input": [{ "role": "user", "content": pass_a_blocks }],
            "text": { "format": { "type": "json_object" } }
        });
        crate::storyboard::provider_trace::append_storyboard_trace(
            "Phase 4a",
            None,
            repair.map(|packet| packet.attempt).unwrap_or(1),
            "request",
            &pass_a_request,
        );
        let pass_a_body = post_model_payload(access, &pass_a_request, Some(STORYBOARD_TIMEOUT))?;
        crate::storyboard::provider_trace::append_storyboard_trace(
            "Phase 4a",
            None,
            repair.map(|packet| packet.attempt).unwrap_or(1),
            "response",
            &serde_json::from_str::<Value>(&pass_a_body)
                .unwrap_or_else(|_| json!({ "raw": pass_a_body })),
        );
        let pass_a_text = model_response_json_text(access, &pass_a_body)
            .ok_or_else(|| "Phase 4a response did not contain JSON.".to_owned())?;
        let batch_picks = parse_phase4_window_picks(&pass_a_text);
        let allowed_orders = batch_shots
            .iter()
            .filter_map(|shot| shot.get("orderIndex").and_then(Value::as_i64))
            .collect::<HashSet<_>>();
        all_picks.extend(
            batch_picks
                .into_iter()
                .filter(|pick| allowed_orders.contains(&pick.order_index)),
        );
    }
    log::info!(
        "Phase 4 pass A attached {pass_a_frames} window midpoint frame(s) across {} batch(es)",
        pass_a_batches.len()
    );
    let picks = all_picks;
    let pick_map = apply_phase4_window_picks(selected, &windows_by_asset, &picks);

    // —— Pass B：选中窗内仍抽满 PHASE4_REFINE_FRAMES，每镜拼成 1 张网格，再按镜批请求 ——
    let mut draft = selected.clone();
    for shot in &mut draft.shots {
        if let Some((window, uncertain)) = pick_map.get(&shot.order_index) {
            let target = shot.duration_ms.max(1).min(window.span_ms().max(1));
            let start = window
                .mid_ms()
                .saturating_sub(target / 2)
                .clamp(window.start_ms, window.end_ms.saturating_sub(1));
            let end = (start + target).min(window.end_ms).max(start + 1);
            shot.source_start_ms = start;
            shot.source_end_ms = end;
            shot.duration_ms = end - start;
            if *uncertain {
                shot.reason = format!("{} [window={} uncertain]", shot.reason, window.window_id);
            } else {
                shot.reason = format!("{} [window={}]", shot.reason, window.window_id);
            }
        }
    }

    let mut ordered_picks = pick_map.iter().collect::<Vec<_>>();
    ordered_picks.sort_by_key(|(order, _)| *order);
    let mut refined = draft.clone();
    let mut pass_b_grids = 0usize;
    let mut pass_b_sample_frames = 0usize;
    for (batch_index, batch) in ordered_picks
        .chunks(PHASE4_REFINE_SHOTS_PER_BATCH)
        .enumerate()
    {
        let batch_orders = batch
            .iter()
            .map(|(order, _)| **order)
            .collect::<HashSet<_>>();
        let mut pass_b_blocks = vec![json!({
            "type": "input_text",
            "text": format!(
                "Brief: {brief}\n\
                Batch {}/{} — refine ONLY these orderIndex values: {:?}. Copy every other shot unchanged from the locked draft.\n\
                Locked storyboard draft (assetIds FINAL; refine ONLY inside each shot's chosen window): {}\n\
                Beat timing (milliseconds; empty means not available): {}\n\
                Chosen windows for this batch: {}\n\
                {feedback_context}\n\n\
                Each attached image is ONE shot's densified window as a left-to-right, top-to-bottom frame grid. Caption lists timesMs in the same order — temporal sample count is unchanged.\n\
                Refine sourceStartMs/sourceEndMs inside that window so the span best matches purpose/requiredVisual.\n\
                Do NOT cut mid spoken phrase in narrationText — prefer natural phrase boundaries.\n\
                Keep durationMs = sourceEndMs - sourceStartMs. No asset swaps, no add/remove/reorder shots.\n\
                No overlapping ranges from the same asset. When beat timing is supplied, the total duration of each beat must equal its endMs-startMs; prefer its verified pausesMs for internal cuts while preserving complete visual actions. Otherwise approach targetDurationMs.\n\
                Divide beat narration across shots when needed.\n\
                For scriptMode=key_message, keep lead-shot onScreenText equal to that beat's onScreenText marker; leave narrationText empty.\n\
                Return complete Storyboard JSON with title, summary, targetDurationMs, scriptMode, beats, uncoveredBeatIds, shots.\n\
                Each shot: cropFocus ([x,y], subject center in the original source frame, normalized 0..1), orderIndex, durationMs, purpose, onScreenText, narrationText, assetId, sourceStartMs, sourceEndMs, reason, beatId, matchLevel, beatPartIndex, beatPartCount.\n\
                Choose cropFocus from the timed frames so the subject remains inside a 9:16 crop throughout the chosen source range. Prefer complete actions and coherent screen direction at adjacent cuts.\n\
                matchLevel must be 'direct' or 'contextual'.",
                batch_index + 1,
                ordered_picks.len().div_ceil(PHASE4_REFINE_SHOTS_PER_BATCH).max(1),
                batch.iter().map(|(order, _)| **order).collect::<Vec<_>>(),
                serde_json::to_string(&refined).unwrap_or_else(|_| "{}".to_owned()),
                serde_json::to_string(&rough.speech_timing).unwrap(),
                serde_json::to_string(
                    &batch
                        .iter()
                        .map(|(order, (window, uncertain))| json!({
                            "orderIndex": order,
                            "windowId": window.window_id,
                            "startMs": window.start_ms,
                            "endMs": window.end_ms,
                            "uncertain": uncertain
                        }))
                        .collect::<Vec<_>>()
                )
                .unwrap_or_else(|_| "[]".to_owned())
            )
        })];
        for (order_index, (window, _)) in batch {
            let Some(source) = selected_sources
                .iter()
                .find(|source| source.asset_id == window.asset_id)
            else {
                continue;
            };
            let Some(path) = source.source_path.as_deref() else {
                continue;
            };
            let times =
                densify_times_in_range(window.start_ms, window.end_ms, PHASE4_REFINE_FRAMES);
            let frames = extract_frames_at_times(
                app,
                &window.asset_id,
                Path::new(path),
                &times,
                &format!("passB_{order_index}"),
            );
            if frames.is_empty() {
                continue;
            }
            pass_b_sample_frames += frames.len();
            let times_ms = frames.iter().map(|(time, _)| *time).collect::<Vec<_>>();
            let frame_paths = frames
                .iter()
                .map(|(_, path)| path.clone())
                .collect::<Vec<_>>();
            let grid_path = frames[0]
                .1
                .parent()
                .map(|parent| parent.join(format!("grid_shot_{order_index}.jpg")));
            let Some(grid_path) = grid_path else {
                continue;
            };
            let Some(grid) = compose_timed_frame_grid(&frame_paths, &grid_path, 3) else {
                continue;
            };
            let Some(image) = read_input_image(&grid) else {
                continue;
            };
            pass_b_blocks.push(json!({
                "type": "input_text",
                "text": format!(
                    "shotOrderIndex={} windowId={} gridColumns=3 timesMs={:?} (read cells L→R, T→B)",
                    order_index, window.window_id, times_ms
                )
            }));
            pass_b_blocks.push(image);
            pass_b_grids += 1;
        }
        let batch_images = pass_b_blocks
            .iter()
            .filter(|block| block.get("type").and_then(Value::as_str) == Some("input_image"))
            .count();
        if batch_images == 0 {
            return Err(format!(
                "Phase 4b batch {} had no readable in-window frame grids.",
                batch_index + 1
            ));
        }
        log::info!(
            "Phase 4 pass B batch {}/{}: refining {} shot(s) as {} grid image(s)",
            batch_index + 1,
            ordered_picks
                .len()
                .div_ceil(PHASE4_REFINE_SHOTS_PER_BATCH)
                .max(1),
            batch_orders.len(),
            batch_images
        );
        let pass_b_request = json!({
            "model": access.custom_config().map(|c| c.model.as_str()).unwrap_or("gpt-5.4"),
            "store": false,
            "stream": true,
            "input": [{ "role": "user", "content": pass_b_blocks }],
            "text": { "format": { "type": "json_object" } }
        });
        crate::storyboard::provider_trace::append_storyboard_trace(
            "Phase 4b",
            None,
            repair.map(|packet| packet.attempt).unwrap_or(1),
            "request",
            &pass_b_request,
        );
        let pass_b_body = post_model_payload(access, &pass_b_request, Some(STORYBOARD_TIMEOUT))?;
        crate::storyboard::provider_trace::append_storyboard_trace(
            "Phase 4b",
            None,
            repair.map(|packet| packet.attempt).unwrap_or(1),
            "response",
            &serde_json::from_str::<Value>(&pass_b_body)
                .unwrap_or_else(|_| json!({ "raw": pass_b_body })),
        );
        let pass_b_text = model_response_json_text(access, &pass_b_body)
            .ok_or_else(|| "Phase 4b response did not contain JSON.".to_owned())?;
        let batch_refined: StoryboardContent = serde_json::from_str(&pass_b_text)
            .map_err(|_| "Phase 4b JSON did not match StoryboardContent schema.".to_owned())?;
        merge_phase4_batch_shots(&mut refined, &batch_refined, &batch_orders, selected);
    }
    log::info!(
        "Phase 4 pass B attached {pass_b_grids} shot-grid image(s) covering {pass_b_sample_frames} timed sample frame(s) across {} batch(es)",
        ordered_picks.len().div_ceil(PHASE4_REFINE_SHOTS_PER_BATCH).max(1)
    );

    // —— Pass C：uncertain 整窗加密；长窗（Pass B 帧间距 >1.5s）围绕精修子区间收窄 ——
    #[derive(Clone, Copy)]
    enum PassCMode {
        Uncertain,
        Narrow,
    }
    let mut pass_c_targets: Vec<(i64, PassCMode)> = Vec::new();
    for (order, (window, uncertain)) in &pick_map {
        if *uncertain {
            pass_c_targets.push((*order, PassCMode::Uncertain));
            continue;
        }
        let spacing = window.span_ms() / PHASE4_REFINE_FRAMES.max(1) as i64;
        if spacing > PHASE4_MAX_FRAME_SPACING_MS {
            pass_c_targets.push((*order, PassCMode::Narrow));
        }
    }
    pass_c_targets.sort_by_key(|(order, _)| *order);
    if !pass_c_targets.is_empty() {
        let uncertain_count = pass_c_targets
            .iter()
            .filter(|(_, mode)| matches!(mode, PassCMode::Uncertain))
            .count();
        let narrow_count = pass_c_targets.len() - uncertain_count;
        log::info!(
            "Phase 4 pass C densifying {} shot(s) (uncertain={}, narrow={})",
            pass_c_targets.len(),
            uncertain_count,
            narrow_count
        );
        for (batch_index, batch) in pass_c_targets
            .chunks(PHASE4_REFINE_SHOTS_PER_BATCH)
            .enumerate()
        {
            let allowed = batch
                .iter()
                .map(|(order, _)| *order)
                .collect::<HashSet<_>>();
            let batch_orders = batch.iter().map(|(order, _)| *order).collect::<Vec<_>>();
            let mut pass_c_blocks = vec![json!({
                "type": "input_text",
                "text": format!(
                    "Re-check ONLY these shots with denser frame grids: {:?}.\n\
                    Batch {}/{}.\n\
                    Current storyboard: {}\n\
                    Keep every other shot unchanged. assetIds stay FINAL.\n\
                    Captions mark UNCERTAIN (full chosen window) or NARROW (tighten inside the listed range only).\n\
                    For NARROW shots, keep sourceStartMs/sourceEndMs inside the NARROW range; prefer complete actions and natural phrase boundaries.\n\
                    Each image is one shot's denser grid; caption lists timesMs L→R, T→B.\n\
                    Return the complete Storyboard JSON after refining only the listed shots' source ranges.\n\
                    Avoid cutting mid spoken phrase.",
                    batch_orders,
                    batch_index + 1,
                    pass_c_targets.len().div_ceil(PHASE4_REFINE_SHOTS_PER_BATCH).max(1),
                    serde_json::to_string(&refined).unwrap_or_else(|_| "{}".to_owned())
                )
            })];
            for (order_index, mode) in batch {
                let Some((window, _)) = pick_map.get(order_index) else {
                    continue;
                };
                let Some(source) = selected_sources
                    .iter()
                    .find(|source| source.asset_id == window.asset_id)
                else {
                    continue;
                };
                let Some(path) = source.source_path.as_deref() else {
                    continue;
                };
                let (sample_start, sample_end, caption_prefix) = match mode {
                    PassCMode::Uncertain => (
                        window.start_ms,
                        window.end_ms,
                        format!(
                            "UNCERTAIN shotOrderIndex={} windowId={}",
                            order_index, window.window_id
                        ),
                    ),
                    PassCMode::Narrow => {
                        let shot = refined
                            .shots
                            .iter()
                            .find(|shot| shot.order_index == *order_index);
                        let Some(shot) = shot else {
                            continue;
                        };
                        let span = (shot.source_end_ms - shot.source_start_ms).max(1);
                        let pad = (span / 4).max(500);
                        let sample_start = shot
                            .source_start_ms
                            .saturating_sub(pad)
                            .max(window.start_ms);
                        let sample_end = (shot.source_end_ms + pad).min(window.end_ms);
                        (
                            sample_start,
                            sample_end.max(sample_start + 1),
                            format!(
                                "NARROW shotOrderIndex={} windowId={} narrowRangeMs=[{},{}]",
                                order_index,
                                window.window_id,
                                sample_start,
                                sample_end.max(sample_start + 1)
                            ),
                        )
                    }
                };
                let times =
                    densify_times_in_range(sample_start, sample_end, PHASE4_UNCERTAIN_FRAMES);
                let frames = extract_frames_at_times(
                    app,
                    &window.asset_id,
                    Path::new(path),
                    &times,
                    &format!("passC_{order_index}"),
                );
                if frames.is_empty() {
                    continue;
                }
                let times_ms = frames.iter().map(|(time, _)| *time).collect::<Vec<_>>();
                let frame_paths = frames
                    .iter()
                    .map(|(_, path)| path.clone())
                    .collect::<Vec<_>>();
                let grid_path = frames[0]
                    .1
                    .parent()
                    .map(|parent| parent.join(format!("grid_passC_{order_index}.jpg")));
                let Some(grid_path) = grid_path else {
                    continue;
                };
                let Some(grid) = compose_timed_frame_grid(&frame_paths, &grid_path, 5) else {
                    continue;
                };
                let Some(image) = read_input_image(&grid) else {
                    continue;
                };
                pass_c_blocks.push(json!({
                    "type": "input_text",
                    "text": format!(
                        "{caption_prefix} gridColumns=5 timesMs={:?} (read cells L→R, T→B)",
                        times_ms
                    )
                }));
                pass_c_blocks.push(image);
            }
            let pass_c_request = json!({
                "model": access.custom_config().map(|c| c.model.as_str()).unwrap_or("gpt-5.4"),
                "store": false,
                "stream": true,
                "input": [{ "role": "user", "content": pass_c_blocks }],
                "text": { "format": { "type": "json_object" } }
            });
            crate::storyboard::provider_trace::append_storyboard_trace(
                "Phase 4c",
                None,
                repair.map(|packet| packet.attempt).unwrap_or(1),
                "request",
                &pass_c_request,
            );
            if let Ok(pass_c_body) =
                post_model_payload(access, &pass_c_request, Some(STORYBOARD_TIMEOUT))
            {
                crate::storyboard::provider_trace::append_storyboard_trace(
                    "Phase 4c",
                    None,
                    repair.map(|packet| packet.attempt).unwrap_or(1),
                    "response",
                    &serde_json::from_str::<Value>(&pass_c_body)
                        .unwrap_or_else(|_| json!({ "raw": pass_c_body })),
                );
                if let Some(text) = model_response_json_text(access, &pass_c_body) {
                    if let Ok(updated) = serde_json::from_str::<StoryboardContent>(&text) {
                        merge_phase4_batch_shots(&mut refined, &updated, &allowed, selected);
                    }
                }
            }
        }
    }

    clamp_shots_to_chosen_windows(&mut refined, &pick_map);
    if rough.speech_timing.beats.is_empty() {
        apply_narration_phrase_duration_floor(&mut refined, &pick_map);
    }
    refined.uncovered_beat_ids = selected.uncovered_beat_ids.clone();
    let mut issues = super::timing::fit_shots(&mut refined, &rough.speech_timing, &pick_map);
    resolve_overlaps_within_chosen_windows(&mut refined, &pick_map);
    issues.extend(collect_phase4_issues(&mut refined, selected));
    refined.brief = brief.to_owned();
    refined.title = rough.title.clone();
    refined.summary = rough.summary.clone();
    refined.target_duration_ms = rough.target_duration_ms;
    refined.script_mode = rough.script_mode.clone();
    refined.beats = rough.beats.clone();
    refined.uncovered_beat_ids = selected.uncovered_beat_ids.clone();
    log::info!(
        "Phase 4 refine complete: shots={}, issues={}, passC={}",
        refined.shots.len(),
        issues.len(),
        pass_c_targets.len()
    );
    Ok((refined, issues))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Phase4WindowPick {
    order_index: i64,
    window_id: String,
    #[serde(default)]
    uncertain: bool,
}

fn parse_phase4_window_picks(text: &str) -> Vec<Phase4WindowPick> {
    let value = serde_json::from_str::<Value>(text).unwrap_or(json!({}));
    let picks = value
        .get("picks")
        .cloned()
        .or_else(|| value.get("windowPicks").cloned())
        .unwrap_or(json!([]));
    serde_json::from_value::<Vec<Phase4WindowPick>>(picks).unwrap_or_default()
}

fn apply_phase4_window_picks(
    selected: &StoryboardContent,
    windows_by_asset: &HashMap<String, Vec<crate::storyboard::multimodal::Phase4ContentWindow>>,
    picks: &[Phase4WindowPick],
) -> HashMap<i64, (crate::storyboard::multimodal::Phase4ContentWindow, bool)> {
    let mut map = HashMap::new();
    for shot in &selected.shots {
        let asset_windows = windows_by_asset
            .get(&shot.asset_id)
            .cloned()
            .unwrap_or_default();
        let pick = picks
            .iter()
            .find(|pick| pick.order_index == shot.order_index);
        let chosen = pick
            .and_then(|pick| {
                asset_windows
                    .iter()
                    .find(|window| window.window_id == pick.window_id)
                    .cloned()
            })
            .or_else(|| {
                asset_windows
                    .iter()
                    .max_by_key(|window| window.span_ms())
                    .cloned()
            });
        if let Some(window) = chosen {
            let uncertain = pick.map(|pick| pick.uncertain).unwrap_or(false);
            map.insert(shot.order_index, (window, uncertain));
        }
    }
    map
}

fn merge_phase4_batch_shots(
    target: &mut StoryboardContent,
    batch: &StoryboardContent,
    allowed_orders: &HashSet<i64>,
    locked: &StoryboardContent,
) {
    for patch in &batch.shots {
        if !allowed_orders.contains(&patch.order_index) {
            continue;
        }
        let Some(original) = locked
            .shots
            .iter()
            .find(|shot| shot.order_index == patch.order_index)
        else {
            continue;
        };
        if patch.asset_id != original.asset_id {
            continue;
        }
        let Some(slot) = target
            .shots
            .iter_mut()
            .find(|shot| shot.order_index == patch.order_index)
        else {
            continue;
        };
        slot.crop_focus = patch.crop_focus;
        slot.source_start_ms = patch.source_start_ms;
        slot.source_end_ms = patch.source_end_ms;
        slot.duration_ms = patch.duration_ms;
        slot.purpose = patch.purpose.clone();
        slot.on_screen_text = patch.on_screen_text.clone();
        slot.narration_text = patch.narration_text.clone();
        slot.reason = patch.reason.clone();
        slot.match_level = patch.match_level.clone();
        slot.beat_part_index = patch.beat_part_index;
        slot.beat_part_count = patch.beat_part_count;
    }
}

fn clamp_shots_to_chosen_windows(
    content: &mut StoryboardContent,
    pick_map: &HashMap<i64, (crate::storyboard::multimodal::Phase4ContentWindow, bool)>,
) {
    for shot in &mut content.shots {
        let Some((window, _)) = pick_map.get(&shot.order_index) else {
            continue;
        };
        if shot.asset_id != window.asset_id {
            continue;
        }
        let win_start = window.start_ms.min(window.end_ms);
        let win_end = window.end_ms.max(window.start_ms);
        if win_end <= win_start {
            continue;
        }
        // 保证 clamp 的 min <= max：终点上限至少比起点多 1ms。
        let start_max = win_end.saturating_sub(1).max(win_start);
        let mut start = shot.source_start_ms.clamp(win_start, start_max);
        let mut end = if shot.source_end_ms > start {
            shot.source_end_ms.min(win_end).max(start + 1)
        } else {
            (start + shot.duration_ms.max(1))
                .min(win_end)
                .max(start + 1)
        };
        if end > win_end {
            end = win_end;
        }
        if end <= start {
            start = win_start;
            end = win_end;
        }
        if end <= start {
            end = start + 1;
        }
        shot.source_start_ms = start;
        shot.source_end_ms = end;
        shot.duration_ms = (end - start).max(1);
    }
}

/// 同素材源范围交叠时，只在 Phase 4 已选内容窗内挪切点；挪过的清 cropFocus。
fn resolve_overlaps_within_chosen_windows(
    content: &mut StoryboardContent,
    pick_map: &HashMap<i64, (crate::storyboard::multimodal::Phase4ContentWindow, bool)>,
) {
    let mut by_asset: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, shot) in content.shots.iter().enumerate() {
        by_asset
            .entry(shot.asset_id.clone())
            .or_default()
            .push(index);
    }
    for indices in by_asset.values() {
        if indices.len() < 2 {
            continue;
        }
        let mut ordered = indices.clone();
        ordered.sort_by_key(|&index| content.shots[index].order_index);
        let mut occupied: Vec<(i64, i64)> = Vec::new();
        for &index in &ordered {
            let (asset_id, order_index, preferred_start, preferred_end, need, window_bounds) = {
                let shot = &content.shots[index];
                let bounds = pick_map.get(&shot.order_index).and_then(|(window, _)| {
                    if window.asset_id != shot.asset_id {
                        return None;
                    }
                    let win_start = window.start_ms.min(window.end_ms);
                    let win_end = window.end_ms.max(window.start_ms);
                    if win_end <= win_start {
                        None
                    } else {
                        Some((win_start, win_end))
                    }
                });
                (
                    shot.asset_id.clone(),
                    shot.order_index,
                    shot.source_start_ms,
                    shot.source_end_ms,
                    shot.duration_ms.max(1),
                    bounds,
                )
            };
            let Some((win_start, win_end)) = window_bounds else {
                occupied.push((preferred_start, preferred_end.max(preferred_start + 1)));
                continue;
            };
            let need = need.clamp(1, (win_end - win_start).max(1));
            let mut start = preferred_start.clamp(win_start, win_end.saturating_sub(1));
            let mut end = preferred_end.min(win_end).max(start + 1);
            if end - start > need {
                end = start + need;
            }
            if ranges_overlap_ms(start, end, &occupied) {
                if let Some((free_start, free_end)) =
                    find_free_in_bounds(win_start, win_end, need, &occupied, start)
                {
                    start = free_start;
                    end = free_end;
                } else if let Some((free_start, free_end)) =
                    find_free_in_bounds(win_start, win_end, 1, &occupied, start)
                {
                    start = free_start;
                    end = free_end;
                }
            }
            let shot = &mut content.shots[index];
            if shot.source_start_ms != start || shot.source_end_ms != end {
                if shot.crop_focus.take().is_some() {
                    log::info!(
                        "Cleared cropFocus for shot_{} after in-window overlap resolve on asset {}",
                        order_index,
                        asset_id
                    );
                }
            }
            shot.source_start_ms = start;
            shot.source_end_ms = end;
            shot.duration_ms = (end - start).max(1);
            occupied.push((start, end));
        }
        // 窗内仍交叠（例如多镜争同一满窗）：按 window 均分，仍不越窗。
        if window_constrained_ranges_overlap(content, indices) {
            pack_overlapping_shots_within_windows(content, indices, pick_map);
        }
    }
}

fn ranges_overlap_ms(start: i64, end: i64, used: &[(i64, i64)]) -> bool {
    used.iter()
        .any(|(used_start, used_end)| start < *used_end && *used_start < end)
}

fn find_free_in_bounds(
    bound_start: i64,
    bound_end: i64,
    need: i64,
    used: &[(i64, i64)],
    preferred_start: i64,
) -> Option<(i64, i64)> {
    let span = bound_end - bound_start;
    if span <= 1 {
        return None;
    }
    let mut intervals = used
        .iter()
        .filter_map(|(start, end)| {
            let start = (*start).max(bound_start);
            let end = (*end).min(bound_end);
            (end > start).then_some((start, end))
        })
        .collect::<Vec<_>>();
    intervals.sort_by_key(|item| item.0);
    let mut cursor = bound_start;
    let mut gaps = Vec::new();
    for (start, end) in intervals {
        if start > cursor {
            gaps.push((cursor, start));
        }
        cursor = cursor.max(end);
    }
    if cursor < bound_end {
        gaps.push((cursor, bound_end));
    }
    let need = need.clamp(1, span);
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

fn window_constrained_ranges_overlap(content: &StoryboardContent, indices: &[usize]) -> bool {
    let ranges = indices
        .iter()
        .map(|&index| {
            let shot = &content.shots[index];
            (shot.source_start_ms, shot.source_end_ms)
        })
        .collect::<Vec<_>>();
    ranges.iter().enumerate().any(|(index, (start, end))| {
        ranges
            .iter()
            .skip(index + 1)
            .any(|(other_start, other_end)| start < other_end && other_start < end)
    })
}

fn pack_overlapping_shots_within_windows(
    content: &mut StoryboardContent,
    indices: &[usize],
    pick_map: &HashMap<i64, (crate::storyboard::multimodal::Phase4ContentWindow, bool)>,
) {
    let mut by_window: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for &index in indices {
        let shot = &content.shots[index];
        let Some((window, _)) = pick_map.get(&shot.order_index) else {
            continue;
        };
        if window.asset_id != shot.asset_id {
            continue;
        }
        let win_start = window.start_ms.min(window.end_ms);
        let win_end = window.end_ms.max(window.start_ms);
        if win_end <= win_start {
            continue;
        }
        by_window
            .entry((win_start, win_end))
            .or_default()
            .push(index);
    }
    for ((win_start, win_end), mut group) in by_window {
        if group.len() < 2 {
            continue;
        }
        group.sort_by_key(|&index| content.shots[index].order_index);
        let span = win_end - win_start;
        let slice = (span / group.len() as i64).max(1);
        let last = group.len() - 1;
        for (offset, &index) in group.iter().enumerate() {
            let start = win_start + offset as i64 * slice;
            let end = if offset == last {
                win_end
            } else {
                (start + slice).min(win_end)
            };
            let shot = &mut content.shots[index];
            let next_start = start.min(win_end.saturating_sub(1));
            let next_end = end.max(next_start + 1).min(win_end);
            if shot.source_start_ms != next_start || shot.source_end_ms != next_end {
                if shot.crop_focus.take().is_some() {
                    log::info!(
                        "Cleared cropFocus for shot_{} after packing overlapping cuts inside window [{}-{}]",
                        shot.order_index,
                        win_start,
                        win_end
                    );
                }
            }
            shot.source_start_ms = next_start;
            shot.source_end_ms = next_end;
            shot.duration_ms = (next_end - next_start).max(1);
        }
    }
}

/// 旁白句界托底：切点落在窗内时，保证画面时长够念完本镜旁白，减轻「话说一半被切」。
fn apply_narration_phrase_duration_floor(
    content: &mut StoryboardContent,
    pick_map: &HashMap<i64, (crate::storyboard::multimodal::Phase4ContentWindow, bool)>,
) {
    for shot in &mut content.shots {
        let narration = shot.narration_text.trim();
        if narration.is_empty() {
            continue;
        }
        let Some((window, _)) = pick_map.get(&shot.order_index) else {
            continue;
        };
        let need_ms = estimated_narration_ms(narration);
        if shot.duration_ms >= need_ms {
            continue;
        }
        let new_end = (shot.source_start_ms + need_ms).min(window.end_ms);
        if new_end > shot.source_end_ms {
            shot.source_end_ms = new_end;
            shot.duration_ms = shot.source_end_ms - shot.source_start_ms;
        }
    }
}

fn estimated_narration_ms(text: &str) -> i64 {
    let mut units = 0.0_f64;
    let mut ascii_run = false;
    let mut cjk = 0.0_f64;
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            if cjk > 0.0 {
                units += (cjk / 2.0).ceil();
                cjk = 0.0;
            }
            if !ascii_run {
                units += 1.0;
                ascii_run = true;
            }
        } else if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            ascii_run = false;
            cjk += 1.0;
            if cjk >= 2.0 {
                units += 1.0;
                cjk = 0.0;
            }
        } else {
            ascii_run = false;
            if cjk > 0.0 {
                units += (cjk / 2.0).ceil();
                cjk = 0.0;
            }
        }
    }
    if cjk > 0.0 {
        units += (cjk / 2.0).ceil();
    }
    ((units.max(1.0)) * 300.0).round() as i64
}

/// 校验 Phase 3 选片输出并收集结构性问题。
fn collect_phase3_issues(
    final_content: &mut StoryboardContent,
    rough: &RoughStoryboard,
) -> Vec<StoryboardIssue> {
    let mut issues = Vec::new();

    let selected_asset_ids = rough
        .shots
        .iter()
        .map(|shot| shot.asset_id.as_str())
        .collect::<std::collections::HashSet<_>>();
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
    let covered_beat_ids: Vec<&str> = if !rough.candidate_pools.is_empty() {
        rough
            .candidate_pools
            .iter()
            .filter(|pool| {
                !final_content
                    .uncovered_beat_ids
                    .iter()
                    .any(|id| id == &pool.beat_id)
            })
            .map(|pool| pool.beat_id.as_str())
            .collect()
    } else {
        rough
            .shots
            .iter()
            .map(|shot| shot.beat_id.as_str())
            .collect()
    };

    if final_content.shots.is_empty() && !covered_beat_ids.is_empty() {
        issues.push(
            StoryboardIssue::new(
                "empty_shot_list",
                "Phase 3 produced an empty shot list for covered beats.",
                true,
            )
            .allowing(vec![
                "pick 2-3 assets from each covered beat's candidate pool",
            ]),
        );
        return issues;
    }

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
                        "Beat order is broken at beat '{beat_id}': next shot belongs to {beat_at_cursor}, but every covered beat must stay contiguous and in order.",
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
                    "keep covered beats in the exact order",
                    "remove shots that were inserted at the wrong position",
                ]),
            );
            break;
        }
        let allowed_assets = match pools_by_beat.get(beat_id) {
            Some(pool) => pool.clone(),
            None => selected_asset_ids.clone(),
        };
        let mut used_in_beat = std::collections::HashSet::new();
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
                            "Shot {} for beat '{beat_id}' uses asset '{}', which is outside that beat's Phase 2 candidate pool.",
                            offset + 1,
                            shot.asset_id
                        ),
                        true,
                    )
                    .for_shots(vec![shot.order_index])
                    .allowing(vec![
                        "pick assetIds only from the beat's candidate pool",
                        "keep the beat at 2-3 shots using distinct pool candidates",
                    ]),
                );
            }
            if !used_in_beat.insert(shot.asset_id.clone()) {
                issues.push(
                    StoryboardIssue::new(
                        "duplicate_asset_in_beat",
                        format!(
                            "Shots of beat '{beat_id}' reuse asset '{}'; each shot needs a distinct candidate.",
                            shot.asset_id
                        ),
                        true,
                    )
                    .for_shots(vec![shot.order_index])
                    .allowing(vec![
                        "replace the duplicated shot with another candidate from the same beat pool",
                    ]),
                );
            }
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
        let pool_candidate_count = pools_by_beat
            .get(beat_id)
            .map(|pool| pool.len())
            .unwrap_or(0);
        if group_count < 2 {
            let affected = final_content
                .shots
                .iter()
                .skip(group_start)
                .take(group_count)
                .map(|shot| shot.order_index)
                .collect::<Vec<_>>();
            if pool_candidate_count >= 2 || (pool_candidate_count == 0 && allowed_assets.len() >= 2)
            {
                issues.push(
                    StoryboardIssue::new(
                        "beat_below_min_shots",
                        format!(
                            "Beat '{beat_id}' has {group_count} shot(s); every covered beat with candidate alternates must use at least 2 distinct pool assets.",
                        ),
                        true,
                    )
                    .for_shots(affected)
                    .allowing(vec![
                        "select 2-3 distinct assetIds from that beat's candidate pool",
                    ]),
                );
            } else if pool_candidate_count == 1 {
                issues.push(
                    StoryboardIssue::new(
                        "beat_below_min_shots_insufficient_pool",
                        format!(
                            "Beat '{beat_id}' has only one Phase 2 candidate, so it cannot yet satisfy the minimum of 2 distinct shots.",
                        ),
                        false,
                    )
                    .for_shots(affected)
                    .allowing(vec![
                        "leave this beat for post-timeline insert_clips repair after storyboard acceptance",
                    ]),
                );
            }
        }
        if group_count > 3 {
            issues.push(
                StoryboardIssue::new(
                    "beat_above_max_shots",
                    format!(
                        "Beat '{beat_id}' has {group_count} shots; keep each beat to 2-3 shots."
                    ),
                    true,
                )
                .for_shots(
                    final_content
                        .shots
                        .iter()
                        .skip(group_start)
                        .take(group_count)
                        .map(|shot| shot.order_index)
                        .collect(),
                )
                .allowing(vec!["drop extra shots until 2-3 remain"]),
            );
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
            .allowing(vec!["remove any shot that is not part of a covered beat"]),
        );
    }

    // 相邻同片硬拒（含跨 beat 衔接）；非相邻复用仍受 40% 上限约束。
    for index in 0..final_content.shots.len().saturating_sub(1) {
        let left = &final_content.shots[index];
        let right = &final_content.shots[index + 1];
        if left.asset_id == right.asset_id {
            issues.push(
                StoryboardIssue::new(
                    "consecutive_duplicate_asset",
                    format!(
                        "Consecutive shots {} and {} both use asset '{}'; adjacent shots must use different assets.",
                        left.order_index, right.order_index, left.asset_id
                    ),
                    true,
                )
                .for_shots(vec![left.order_index, right.order_index])
                .allowing(vec![
                    "replace one of the adjacent shots with a different assetId from its beat pool",
                    "keep non-adjacent reuse only within the diversity limit",
                ]),
            );
        }
    }

    // 全序列 40% 复用上限：在 Phase 3 就拦，避免 Phase 5 失败后空转 Phase 4。
    if final_content.shots.len() >= 2 {
        let max_allowed = super::max_asset_uses_for_shot_count(final_content.shots.len());
        let mut asset_usage: HashMap<&str, Vec<i64>> = HashMap::new();
        for shot in &final_content.shots {
            asset_usage
                .entry(shot.asset_id.as_str())
                .or_default()
                .push(shot.order_index);
        }
        for (asset_id, shot_indices) in asset_usage {
            let count = shot_indices.len();
            if count > max_allowed {
                let percentage = count * 100 / final_content.shots.len();
                let excess = count - max_allowed;
                issues.push(
                    StoryboardIssue::new(
                        "asset_over_diversity_limit",
                        format!(
                            "Asset '{asset_id}' appears in {count} of {} shots ({percentage}%), exceeding the 40% diversity limit of {max_allowed} shots.",
                            final_content.shots.len()
                        ),
                        true,
                    )
                    .for_shots(shot_indices)
                    .allowing(vec![
                        format!(
                            "replace {excess} of these shots with different assetIds from their beat pools so '{asset_id}' is used at most {max_allowed} times"
                        ),
                        "or mark the weakest beat uncovered=true when pools cannot supply enough distinct assets".to_owned(),
                    ]),
                );
            }
        }
    }

    final_content.title = rough.title.clone();
    final_content.summary = rough.summary.clone();
    final_content.target_duration_ms = rough.target_duration_ms;
    final_content.script_mode = rough.script_mode.clone();
    final_content.beats = rough.beats.clone();
    // uncovered：Phase 3 可诚实追加；保留 Phase 2 已有项。
    let mut uncovered = rough.uncovered_beat_ids.clone();
    for id in &final_content.uncovered_beat_ids {
        if !uncovered.iter().any(|existing| existing == id) {
            uncovered.push(id.clone());
        }
    }
    final_content.uncovered_beat_ids = uncovered;
    issues
}

/// Phase 4：禁止换片；其余沿用选片结构约束的机械标准化。
pub(crate) fn collect_phase4_issues(
    refined: &mut StoryboardContent,
    selected: &StoryboardContent,
) -> Vec<StoryboardIssue> {
    let mut issues = Vec::new();
    if refined.shots.len() != selected.shots.len() {
        issues.push(
            StoryboardIssue::new(
                "shot_count_changed",
                format!(
                    "Phase 4 changed shot count from {} to {}; keep the locked selection length.",
                    selected.shots.len(),
                    refined.shots.len()
                ),
                true,
            )
            .allowing(vec![
                "restore the exact locked shot list and only edit ranges/narration",
            ]),
        );
    }
    let limit = refined.shots.len().min(selected.shots.len());
    for index in 0..limit {
        let locked = &selected.shots[index];
        let shot = &refined.shots[index];
        if shot.asset_id != locked.asset_id || shot.beat_id != locked.beat_id {
            issues.push(
                StoryboardIssue::new(
                    "asset_swapped",
                    format!(
                        "Phase 4 changed shot {} from asset '{}' (beat '{}') to '{}' (beat '{}'). assetIds are locked.",
                        locked.order_index,
                        locked.asset_id,
                        locked.beat_id,
                        shot.asset_id,
                        shot.beat_id
                    ),
                    true,
                )
                .for_shots(vec![locked.order_index])
                .allowing(vec![
                    "restore the locked assetId and beatId",
                    "only adjust sourceStartMs/sourceEndMs/durationMs/narrationText",
                ]),
            );
        }
    }
    // 标准化子镜头字段
    let mut cursor = 0usize;
    while cursor < refined.shots.len() {
        let beat_id = refined.shots[cursor].beat_id.clone();
        let start = cursor;
        while cursor < refined.shots.len() && refined.shots[cursor].beat_id == beat_id {
            cursor += 1;
        }
        let count = cursor - start;
        for (offset, shot) in refined
            .shots
            .iter_mut()
            .enumerate()
            .take(cursor)
            .skip(start)
        {
            let part_index = (offset - start + 1) as i64;
            shot.beat_part_index = part_index;
            shot.beat_part_count = count as i64;
            shot.split_role = if count <= 1 {
                "lead".to_owned()
            } else if part_index == 1 {
                "lead".to_owned()
            } else if part_index == count as i64 {
                "tail".to_owned()
            } else {
                "bridge".to_owned()
            };
            shot.order_index = (offset as i64) + 1;
        }
    }
    refined.title = selected.title.clone();
    refined.summary = selected.summary.clone();
    refined.target_duration_ms = selected.target_duration_ms;
    refined.script_mode = selected.script_mode.clone();
    refined.beats = selected.beats.clone();
    refined.uncovered_beat_ids = selected.uncovered_beat_ids.clone();
    issues
}
