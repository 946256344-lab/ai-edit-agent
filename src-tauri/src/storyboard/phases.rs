// storyboard/phases.rs - Storyboard 分步生成
//
// Phase 1: 叙事结构（注入本地库库存摘要，约束 requiredVisual/visualKeywords）
// Phase 2: 全库段排序，每 beat 取 9 段；综合分与分项高分比例按项目设置调整
// Phase 3: 每拍单独看最多 9 张图，默认一镜；9 条均可选；网格按该条时间窗现拼
// Phase 4: 锁在 P3 窗内精修切点（禁止换片；整片也不再 Pass A 另切窗）
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
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;

const PHASE2_POOL_SIZE: usize = 9;
/// 每个 beat 池内同一素材最多几段。
const PHASE2_MAX_SEGMENTS_PER_ASSET_IN_POOL: usize = 2;
/// 每个 beat 池内互为相似的段最多几条。
const PHASE2_MAX_SIMILAR_IN_POOL: usize = 2;
/// 证据向量余弦相似度达到该阈值视为「相似素材」，入池时互斥。
const PHASE2_SIMILARITY_COSINE: f64 = 0.92;
/// 视觉/OCR 标签 Jaccard 重叠达到该阈值视为相似。
const PHASE2_SIMILARITY_TAG_JACCARD: f64 = 0.55;
/// 与 preview 相同：24×24 灰度均差低于该值视为画面相似。
const PHASE2_PIXEL_DIFF_SIMILAR: f64 = 12.0;
/// Phase 1 库存摘要字符上限，避免挤占 brief。
const INVENTORY_MAX_CHARS: usize = 3_500;
const INVENTORY_TOP_TAGS: usize = 48;
const INVENTORY_TOP_SCENES: usize = 24;
const INVENTORY_TOP_OCR: usize = 16;

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
    /// 用户对单镜长短的偏好（short / default / long）；旧记录与模型瞎写都落回 default。
    #[serde(default)]
    pub shot_length_hint: String,
    pub beats: Vec<StoryboardBeat>,
}

/// 单镜长短偏好：只影响 Phase 3 的每拍时长上下界与提示词，不改任何硬约束。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShotLengthHint {
    Short,
    Default,
    Long,
}

impl ShotLengthHint {
    /// 每拍成片时长的下限/上限（毫秒）。Default 逐字等于改造前的 clamp(1_200, 6_000)。
    pub(crate) fn duration_bounds(self) -> (i64, i64) {
        match self {
            ShotLengthHint::Short => (900, 2_500),
            ShotLengthHint::Default => (1_200, 6_000),
            ShotLengthHint::Long => (2_000, 8_000),
        }
    }

    /// 给模型看的节奏说明；Default 保持原文案，避免旧行为漂移。
    pub(crate) fn prompt_line(self) -> &'static str {
        match self {
            ShotLengthHint::Short => {
                "The user asked for fast cuts. Prefer the shortest clip that still covers requiredVisual. \
                A second distinct shot is still allowed only when two clips honestly cover different parts of requiredVisual — do not pad."
            }
            ShotLengthHint::Default => "Picture shots should last about 2-3 seconds.",
            ShotLengthHint::Long => {
                "The user asked for longer, calmer shots. Prefer ONE clip held longer; do not split this beat \
                unless two clips honestly cover different parts of requiredVisual."
            }
        }
    }
}

/// 宽松解析模型的 shotLengthHint：不认识的值不报错、不打回重试，落回 default。
pub(crate) fn normalize_shot_length_hint(value: &str) -> ShotLengthHint {
    match value.trim().to_ascii_lowercase().as_str() {
        "short" | "fast" | "quick" | "snappy" => ShotLengthHint::Short,
        "long" | "slow" | "calm" | "relaxed" => ShotLengthHint::Long,
        _ => ShotLengthHint::Default,
    }
}

/// Phase 2 输出：粗略 storyboard（每个 beat 一个 shot）
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BeatCandidatePool {
    pub(crate) beat_id: String,
    pub(crate) beat_purpose: String,
    pub(crate) candidates: Vec<StoryboardSource>,
    /// 与 candidates 一一对应的本地召回分数；旧 pools_json 缺省为空。
    #[serde(default)]
    pub(crate) scores: Vec<scoring::CandidateScore>,
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
    /// 用户对单镜长短的偏好，从 Phase 1 透传到 Phase 3 算时长；旧记录缺省为 default。
    #[serde(default)]
    pub(crate) shot_length_hint: String,
    pub beats: Vec<StoryboardBeat>,
    pub uncovered_beat_ids: Vec<String>,
    pub shots: Vec<StoryboardShot>,
    /// 每个已覆盖 beat 的 Phase 2 预选候选池。仅用于内存中的 Phase 3 素材约束，
    /// 不序列化进 prompt（避免请求体过大）；模型只看到下面的精简候选卡片。
    #[serde(default, skip_serializing)]
    pub candidate_pools: Vec<BeatCandidatePool>,
}

/// Phase 1: 生成叙事结构（scriptMode 由调用方在请求前锁定）。
/// `library_inventory` 为本地视觉/OCR 库存摘要；空串时退回仅凭 brief 写结构。
pub(crate) fn phase1_generate_narrative(
    access: &ModelAccess,
    brief: &str,
    required_script_mode: &str,
    library_inventory: &str,
    feedback: Option<&str>,
    voiceover_duration_ms: Option<i64>,
    requested_duration_ms: Option<i64>,
) -> Result<NarrativeStructure, String> {
    log::info!(
        "Phase 1: Generating narrative structure from brief (required_script_mode={required_script_mode}, inventory_chars={})",
        library_inventory.chars().count()
    );

    let feedback_context = feedback.map_or(String::new(), |value| {
        format!(
            "\n\nPrevious attempt was too coarse or under-covered the brief: {value}\nRevise the beat segmentation to be finer-grained and cover every distinct idea in the brief."
        )
    });

    let mut mode_instructions = if required_script_mode == "full_script" {
        "REQUIRED scriptMode=full_script (the brief is the spoken narration to read). Do not choose key_message.\n\
        Put the exact speakable script into spokenScript (strip only non-spoken instructions like 'please edit a video'; keep wording, order, and language unchanged — do not paraphrase, summarize, or invent). Split the SAME wording across beat.narration fields so concatenating beat narrations (with spaces) reproduces spokenScript without extras or omissions. Set each beat.onScreenText to \"\" (subtitles come from voice alignment later).\n\
        targetDurationMs must match the real spokenScript length; never invent a longer essay than spokenScript. Split by meaning, not punctuation. Each beat should cover one distinct spoken idea — about 2-3 seconds of speech; duration can wobble. One picture shot is selected per beat."
            .to_owned()
    } else {
        "REQUIRED scriptMode=key_message (locked by the system because the brief is a goal/outline/theme, not a spoken script). Do not choose full_script.\n\
        spokenScript=\"\", set every beat.narration to \"\", and set every beat.onScreenText to \"\" unless the user explicitly asked for on-screen titles.\n\
        Follow the user's duration if they named one (3-120 seconds). \
        If they did not: suggest 15-45 seconds. Split information into beats of one distinct idea each — about 2-3 seconds worth — rather than collapsing a whole product act into one beat. \
        Prefer a focused promo over a 60-90s essay; do not pad empty time. This is guidance, not a hard cap."
            .to_owned()
    };
    // shotLengthHint：用户对单镜长短的原话偏好。只在这一维表达，别替用户决定节奏。
    mode_instructions.push_str(
        "\nshotLengthHint: \"short\" when the user asks for fast cuts / 快节奏 / 别拖 / 多切几个镜头, \
        \"long\" when they ask for longer or calmer shots / 慢一点 / 长镜头 / 别切太碎, \
        otherwise \"default\". Judge this from the user's own words about pacing; do not guess from the topic.",
    );
    if let Some(duration_ms) = voiceover_duration_ms {
        let beats = super::expected_beat_count(duration_ms);
        let secs = (duration_ms + 500) / 1000;
        mode_instructions.push_str(&format!(
            "\nA voiceover was already synthesized at {duration_ms}ms (~{secs}s). Set targetDurationMs to {duration_ms}. Write about {beats} beats (2-3 seconds of speech each), not 3-5 thematic acts. One picture shot is cut per beat, so a 6-second beat becomes a 6-second hold. Split spokenScript by meaning across beats so concatenating them still reproduces the script; Rust will map beats onto the TTS timestamps."
        ));
    } else if let Some(duration_ms) = requested_duration_ms {
        let beats = super::expected_beat_count(duration_ms);
        mode_instructions.push_str(&format!(
            "\nThe user asked for a finished video of {duration_ms}ms. Set targetDurationMs to {duration_ms}. Write about {beats} beats of about 2-3 seconds. One picture shot is cut per beat."
        ));
    }

    let inventory_block = if library_inventory.trim().is_empty() {
        String::new()
    } else {
        format!(
            "\n\nLIBRARY INVENTORY (local visual/OCR evidence already analyzed in this project — not filenames):\n{library_inventory}\n\
            Inventory rules (must follow):\n\
            - Keep the brief's narrative intent, but write requiredVisual and visualKeywords so they can plausibly be shot from this inventory.\n\
            - Prefer concrete English keywords that overlap inventory tags/scenes/OCR (e.g. if inventory has \"certificate wall\" / \"framed plaques\", do NOT invent \"signed warranty contract with stamp\").\n\
            - If the brief needs a visual the inventory does not support, keep the beat purpose honest and set requiredVisual to the closest available evidence type; never fabricate subjects absent from the inventory just to fill every beat.\n\
            - Do not select or invent asset IDs here — media selection happens later."
        )
    };

    let prompt = format!(
        "Analyze this brief and create a narrative structure: {brief}\n\
        Return a JSON with: title, summary, targetDurationMs (3-120 seconds), scriptMode (must be \"{required_script_mode}\"), spokenScript (string), shotLengthHint (string), and beats.\n\
        Each beat must contain: id (unique short slug), purpose (one sentence), requiredVisual (specific visual requirement grounded in the library inventory when provided), visualKeywords (array of 4-8 concrete English nouns/verbs naming what should be visible on screen — no abstract words; asset tags are English), narration (string), onScreenText (string).\n\
        {mode_instructions}\n\
        Use beat segmentation to express separate information points, not broad paragraph chunks. One beat should cover one concrete idea, action, or emotional turn — split here rather than collapsing a whole product act (intro / problem / proof / CTA) into one beat. One picture shot is selected per beat, so a 6-second beat produces a 6-second hold.\n\
        Determine the appropriate number of beats from distinct information points. Do not select any media yet — this stage is pure story structure.\n\
        targetDurationMs priority: if the system already gave you a fixed duration above (voiceover pre-synthesized, or user named one), copy it exactly — do not invent a different value. Otherwise propose a duration consistent with the script length (full_script) or the 15-45s guidance (key_message).\n\
        {feedback_context}{inventory_block}"
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

/// 从已加载的整片候选汇总本地库存，供 Phase 1 约束画面写法（不发送路径/ID）。
pub(crate) fn build_library_inventory_summary(sources: &[StoryboardSource]) -> String {
    let total = sources.len();
    let mut with_visual = 0usize;
    let mut tag_counts: HashMap<String, usize> = HashMap::new();
    let mut scene_counts: HashMap<String, usize> = HashMap::new();
    let mut ocr_counts: HashMap<String, usize> = HashMap::new();

    for source in sources {
        let mut saw_visual = false;
        for evidence in &source.visual_evidence {
            saw_visual = true;
            for tag in evidence
                .subjects
                .iter()
                .chain(evidence.actions.iter())
                .chain(evidence.products.iter())
            {
                if let Some(normalized) = normalize_inventory_phrase(tag) {
                    *tag_counts.entry(normalized).or_insert(0) += 1;
                }
            }
            if let Some(scene) = evidence
                .scene
                .as_deref()
                .and_then(normalize_inventory_phrase)
            {
                *scene_counts.entry(scene).or_insert(0) += 1;
            }
            if let Some(role) = evidence
                .narrative_role
                .as_deref()
                .and_then(normalize_inventory_phrase)
            {
                *tag_counts.entry(role).or_insert(0) += 1;
            }
        }
        for segment in &source.scene_segments {
            if let Some(evidence) = &segment.visual_evidence {
                saw_visual = true;
                for tag in evidence
                    .subjects
                    .iter()
                    .chain(evidence.actions.iter())
                    .chain(evidence.products.iter())
                {
                    if let Some(normalized) = normalize_inventory_phrase(tag) {
                        *tag_counts.entry(normalized).or_insert(0) += 1;
                    }
                }
                if let Some(scene) = evidence
                    .scene
                    .as_deref()
                    .and_then(normalize_inventory_phrase)
                {
                    *scene_counts.entry(scene).or_insert(0) += 1;
                }
                if let Some(role) = evidence
                    .narrative_role
                    .as_deref()
                    .and_then(normalize_inventory_phrase)
                {
                    *tag_counts.entry(role).or_insert(0) += 1;
                }
            }
        }
        if saw_visual {
            with_visual += 1;
        }
        for item in &source.ocr_evidence {
            if !ocr_is_meaningful(&item.text) {
                continue;
            }
            if let Some(normalized) = normalize_inventory_ocr(&item.text) {
                *ocr_counts.entry(normalized).or_insert(0) += 1;
            }
        }
    }

    if tag_counts.is_empty() && scene_counts.is_empty() && ocr_counts.is_empty() {
        return format!(
            "videos={total}; withVisualEvidence={with_visual}; tags=(none yet — write conservative requiredVisual, avoid inventing documents/labels not known to exist)."
        );
    }

    let tags = top_count_lines(&tag_counts, INVENTORY_TOP_TAGS);
    let scenes = top_count_lines(&scene_counts, INVENTORY_TOP_SCENES);
    let ocr = top_count_lines(&ocr_counts, INVENTORY_TOP_OCR);

    let mut summary = format!(
        "videos={total}; withVisualEvidence={with_visual}; withoutVisualEvidence={}\n",
        total.saturating_sub(with_visual)
    );
    if !tags.is_empty() {
        summary.push_str("frequentVisualTags: ");
        summary.push_str(&tags.join("; "));
        summary.push('\n');
    }
    if !scenes.is_empty() {
        summary.push_str("frequentScenes: ");
        summary.push_str(&scenes.join("; "));
        summary.push('\n');
    }
    if !ocr.is_empty() {
        summary.push_str("sampleOcr: ");
        summary.push_str(&ocr.join("; "));
        summary.push('\n');
    }

    trim_inventory_chars(summary, INVENTORY_MAX_CHARS)
}

fn normalize_inventory_phrase(raw: &str) -> Option<String> {
    let trimmed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = trimmed.trim();
    if trimmed.chars().count() < 2 {
        return None;
    }
    let lowered = trimmed.to_lowercase();
    if lowered.chars().count() > 64 {
        return Some(lowered.chars().take(64).collect());
    }
    Some(lowered)
}

fn normalize_inventory_ocr(raw: &str) -> Option<String> {
    let phrase = normalize_inventory_phrase(raw)?;
    if phrase.chars().count() > 40 {
        return Some(phrase.chars().take(40).collect());
    }
    Some(phrase)
}

fn top_count_lines(counts: &HashMap<String, usize>, limit: usize) -> Vec<String> {
    let mut items = counts.iter().collect::<Vec<_>>();
    items.sort_by(|left, right| right.1.cmp(left.1).then_with(|| left.0.cmp(right.0)));
    items
        .into_iter()
        .take(limit)
        .map(|(phrase, count)| format!("{phrase}×{count}"))
        .collect()
}

fn trim_inventory_chars(text: String, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text;
    }
    let mut clipped: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    clipped.push('…');
    clipped
}

/// Phase 2: 全库段排序后每个 beat 取 9 段；不调用模型选镜。
pub(crate) fn phase2_rough_shot_selection(
    narrative: &NarrativeStructure,
    sources: &[StoryboardSource],
    usage_counts: &HashMap<String, i32>,
    embeddings: &[Vec<f32>],
    clip_embeddings: &[Vec<f32>],
    speech_timing: super::timing::SpeechTiming,
    score_first_slots: usize,
) -> Result<RoughStoryboard, String> {
    log::info!(
        "Phase 2: Shortlisting {} segments for {} beats (max {} per asset, max {} similar)",
        PHASE2_POOL_SIZE,
        narrative.beats.len(),
        PHASE2_MAX_SEGMENTS_PER_ASSET_IN_POOL,
        PHASE2_MAX_SIMILAR_IN_POOL
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
        let beat_clip = clip_embeddings.get(beat_index);
        let target_each = speech_timing.duration(&beat.id).unwrap_or(target_each);
        let ranked = scoring::rank_segment_candidates(
            sources.to_vec(),
            beat,
            target_each,
            &[],
            usage_counts,
            beat_embedding.map(Vec::as_slice),
            beat_clip.map(Vec::as_slice),
        );
        let (pool, scores, library_exhausted) =
            pick_segment_pool(&ranked, PHASE2_POOL_SIZE, score_first_slots);
        let sample = pool
            .iter()
            .zip(scores.iter())
            .enumerate()
            .map(|(index, (candidate, score))| {
                let segment = candidate
                    .segment
                    .as_ref()
                    .map(|segment| segment.id.as_str())
                    .unwrap_or("-");
                format!(
                    "#{index}:{}/{segment}(sem={:.1}/lex={:.1}/clip={:.1}/q={:.1}/d={:.1}/f={:.1}=total={:.1}{})",
                    candidate.asset_id,
                    score.semantic,
                    score.lexical,
                    score.clip,
                    score.quality,
                    score.duration,
                    score.freshness,
                    score.total,
                    if score.has_evidence { "" } else { ",noEv" }
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        log::info!(
            "Beat '{}': poolSize={}, libraryExhausted={}, sample={}",
            beat.id,
            pool.len(),
            library_exhausted,
            sample
        );
        crate::storyboard::provider_trace::append_pool_trace(
            "Phase 2",
            &beat.id,
            &json!({
                "purpose": beat.purpose,
                "narration": beat.narration,
                "requiredVisual": beat.required_visual,
                "visualKeywords": beat.visual_keywords,
                "libraryExhausted": library_exhausted,
                "candidates": pool_trace_candidates(&pool, &scores),
            }),
        );
        if pool.is_empty() {
            log::warn!(
                "Beat '{}': no usable segment candidates; leaving uncovered",
                beat.id
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
            "storyboard_phase2_empty: no beat received a usable segment candidate.".to_owned(),
        );
    }

    log::info!(
        "Phase 2 complete: {} covered beat pools, {} uncovered beats",
        candidate_pools.len(),
        uncovered_beat_ids.len()
    );

    // 为下游 Phase 3 提供每 beat 的临时主镜（池内第一条）；真正 1–3 选片仍由 Phase 3 完成。
    let mut shots = Vec::new();
    for (index, pool) in candidate_pools.iter().enumerate() {
        let Some(main) = pool.candidates.first() else {
            continue;
        };
        let beat = narrative.beats.iter().find(|beat| beat.id == pool.beat_id);
        let (range_start, range_end) = candidate_range_ms(main);
        let span = (range_end - range_start).max(1);
        let duration = speech_timing
            .duration(&pool.beat_id)
            .unwrap_or(target_each)
            .max(1)
            .min(span);
        shots.push(StoryboardShot {
            crop_focus: None,
            order_index: (index as i64) + 1,
            duration_ms: duration,
            purpose: pool.beat_purpose.clone(),
            on_screen_text: String::new(),
            narration_text: beat.map(|b| b.narration.clone()).unwrap_or_default(),
            asset_id: main.asset_id.clone(),
            source_start_ms: range_start,
            source_end_ms: range_start + duration,
            reason: "Phase 2 shortlist lead candidate.".to_owned(),
            beat_id: pool.beat_id.clone(),
            match_level: "contextual".to_owned(),
            beat_part_index: 1,
            beat_part_count: 1,
            split_role: "lead".to_owned(),
            segment_id: main.segment.as_ref().map(|segment| segment.id.clone()),
        });
    }

    Ok(RoughStoryboard {
        speech_timing,
        title: narrative.title.clone(),
        summary: narrative.summary.clone(),
        target_duration_ms: narrative.target_duration_ms,
        script_mode: narrative.script_mode.clone(),
        shot_length_hint: narrative.shot_length_hint.clone(),
        beats: narrative.beats.clone(),
        uncovered_beat_ids,
        shots,
        candidate_pools,
    })
}

/// 按排名取 `target_len` 段：同片最多 2 段，相似最多 2 条，重叠同片直接丢掉。
fn pick_segment_pool(
    ranked: &[scoring::ScoredCandidate],
    target_len: usize,
    score_first_slots: usize,
) -> (Vec<StoryboardSource>, Vec<scoring::CandidateScore>, bool) {
    let mut pool: Vec<StoryboardSource> = Vec::new();
    let mut scores: Vec<scoring::CandidateScore> = Vec::new();
    let mut per_asset: HashMap<String, usize> = HashMap::new();
    for candidate in ranked {
        if pool.len() >= score_first_slots.min(target_len) {
            break;
        }
        add_pool_candidate(candidate, &mut pool, &mut scores, &mut per_asset);
    }
    let mut component_orders = [
        ranked.iter().collect::<Vec<_>>(),
        ranked.iter().collect::<Vec<_>>(),
        ranked.iter().collect::<Vec<_>>(),
    ];
    component_orders[0].sort_by(|a, b| b.score.clip.total_cmp(&a.score.clip));
    component_orders[1].sort_by(|a, b| b.score.semantic.total_cmp(&a.score.semantic));
    component_orders[2].sort_by(|a, b| b.score.lexical.total_cmp(&a.score.lexical));
    while pool.len() < target_len {
        let mut added = false;
        for (channel, order) in component_orders.iter().enumerate() {
            if pool.len() >= target_len {
                break;
            }
            for candidate in order {
                let component = match channel {
                    0 => candidate.score.clip,
                    1 => candidate.score.semantic,
                    _ => candidate.score.lexical,
                };
                if component > 0.0
                    && add_pool_candidate(candidate, &mut pool, &mut scores, &mut per_asset)
                {
                    added = true;
                    break;
                }
            }
        }
        if !added {
            break;
        }
    }
    for candidate in ranked {
        if pool.len() >= target_len {
            break;
        }
        add_pool_candidate(candidate, &mut pool, &mut scores, &mut per_asset);
    }
    let library_exhausted = pool.len() < target_len;
    (pool, scores, library_exhausted)
}

fn add_pool_candidate(
    candidate: &scoring::ScoredCandidate,
    pool: &mut Vec<StoryboardSource>,
    scores: &mut Vec<scoring::CandidateScore>,
    per_asset: &mut HashMap<String, usize>,
) -> bool {
    let source = &candidate.source;
    if pool.iter().any(|existing| {
        existing.asset_id == source.asset_id
            && ranges_overlap(candidate_range_ms(existing), candidate_range_ms(source))
    }) {
        return false;
    }
    let count = *per_asset.get(&source.asset_id).unwrap_or(&0);
    if count >= PHASE2_MAX_SEGMENTS_PER_ASSET_IN_POOL {
        return false;
    }
    let similar = pool
        .iter()
        .filter(|existing| sources_are_similar(existing, source))
        .count();
    if similar >= PHASE2_MAX_SIMILAR_IN_POOL {
        return false;
    }
    per_asset.insert(source.asset_id.clone(), count + 1);
    pool.push(source.clone());
    scores.push(candidate.score.clone());
    true
}

fn candidate_range_ms(source: &StoryboardSource) -> (i64, i64) {
    if let Some(segment) = &source.segment {
        (segment.start_ms, segment.end_ms)
    } else {
        (0, source.duration_ms.unwrap_or(0).max(0))
    }
}

fn ranges_overlap(a: (i64, i64), b: (i64, i64)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

pub(crate) fn sources_are_similar(left: &StoryboardSource, right: &StoryboardSource) -> bool {
    if left.asset_id == right.asset_id {
        // 同素材：范围重叠才判重（允许同片不同段同池）。
        if ranges_overlap(candidate_range_ms(left), candidate_range_ms(right)) {
            return true;
        }
    }
    if still_frames_are_similar(left, right) {
        return true;
    }
    let left_emb = left
        .segment_embedding
        .as_deref()
        .or(left.evidence_embedding.as_deref());
    let right_emb = right
        .segment_embedding
        .as_deref()
        .or(right.evidence_embedding.as_deref());
    if let (Some(a), Some(b)) = (left_emb, right_emb) {
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

fn still_frames_are_similar(left: &StoryboardSource, right: &StoryboardSource) -> bool {
    let Some(left_sig) = source_still_signature(left) else {
        return false;
    };
    let Some(right_sig) = source_still_signature(right) else {
        return false;
    };
    mean_pixel_difference(&left_sig, &right_sig)
        .is_some_and(|difference| difference < PHASE2_PIXEL_DIFF_SIMILAR)
}

fn source_still_path(source: &StoryboardSource) -> Option<&str> {
    if let Some(segment) = &source.segment {
        if let Some(path) = segment
            .frame_paths
            .iter()
            .map(String::as_str)
            .find(|path| !path.is_empty())
        {
            return Some(path);
        }
    }
    if let Some(path) = source
        .keyframes
        .iter()
        .map(|frame| frame.image_path.as_str())
        .find(|path| !path.is_empty())
    {
        return Some(path);
    }
    source
        .scene_segments
        .iter()
        .flat_map(|segment| segment.frames.iter())
        .map(|frame| frame.image_path.as_str())
        .find(|path| !path.is_empty())
}

fn source_still_signature(source: &StoryboardSource) -> Option<Vec<u8>> {
    let path = source_still_path(source)?;
    cached_still_signature(Path::new(path))
}

fn cached_still_signature(path: &Path) -> Option<Vec<u8>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Vec<u8>>>>> = OnceLock::new();
    let key = path.to_string_lossy().into_owned();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some(existing) = guard.get(&key) {
            return existing.clone();
        }
    }
    let computed = still_signature_from_path(path);
    if let Ok(mut guard) = cache.lock() {
        guard.insert(key, computed.clone());
    }
    computed
}

fn still_signature_from_path(path: &Path) -> Option<Vec<u8>> {
    let gray = image::open(path).ok()?.to_luma8();
    let resized = image::imageops::resize(&gray, 24, 24, image::imageops::FilterType::Triangle);
    Some(resized.into_raw())
}

fn mean_pixel_difference(first: &[u8], second: &[u8]) -> Option<f64> {
    (first.len() == second.len() && !first.is_empty()).then(|| {
        first
            .iter()
            .zip(second)
            .map(|(left, right)| left.abs_diff(*right) as u64)
            .sum::<u64>() as f64
            / first.len() as f64
    })
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
    let evidence = candidate_evidence_summary(source);
    let stored_grid_readable = source
        .keyframe_grid_path
        .as_deref()
        .is_some_and(|path| Path::new(path).is_file());
    let usable_ms = source
        .segment
        .as_ref()
        .map(|segment| segment.span_ms())
        .or(source.duration_ms);
    let mut card = json!({
        "candidateIndex": index,
        "assetId": source.asset_id,
        "kind": source.kind,
        "durationMs": source.segment.as_ref().map(|segment| segment.span_ms()).or(source.duration_ms),
        "usableMs": usable_ms,
        "hasKeyframeGrid": keyframe_grid_attached || stored_grid_readable,
        "keyframeGridAttached": keyframe_grid_attached,
        "keyframeTimesMs": source.keyframes.iter().map(|frame| frame.time_ms).collect::<Vec<_>>(),
        "sceneSegments": source.scene_segments.iter().take(8).map(|segment| {
            json!({"id": segment.id, "startMs": segment.start_ms, "endMs": segment.end_ms})
        }).collect::<Vec<_>>(),
        "visualTags": visual_tags,
        "visibleCaption": evidence.get("caption").cloned().unwrap_or(Value::Null),
        "narrativeRole": evidence.get("narrativeRole").cloned().unwrap_or(Value::Null),
        "scene": evidence.get("scene").cloned().unwrap_or(Value::Null),
        "subjects": evidence.get("subjects").cloned().unwrap_or_else(|| json!([])),
        "actions": evidence.get("actions").cloned().unwrap_or_else(|| json!([])),
    });
    if let Some(segment) = &source.segment {
        card["segmentId"] = json!(segment.id);
        card["startMs"] = json!(segment.start_ms);
        card["endMs"] = json!(segment.end_ms);
        card["shotType"] = json!(segment.shot_type);
        card["cameraMotion"] = json!(segment.camera_motion);
    }
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

/// 每个 beat 的完整短名单卡片（目标 9 段），供 Phase 3 选片。
fn phase3_pool_cards(
    pools: &[BeatCandidatePool],
    beats: &[StoryboardBeat],
    attached_indexes: &HashSet<usize>,
    speech_timing: &super::timing::SpeechTiming,
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
                        attached_indexes.contains(&index),
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
                "narrationMs": speech_timing.duration(&pool.beat_id),
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
        parse_beat_pick, phase2_rough_shot_selection, phase3_candidate_cards, phase3_pool_cards,
        pick_segment_pool, resolve_overlaps_within_chosen_windows,
        resolve_overlaps_within_chosen_windows_scoped, BeatCandidatePool, NarrativeStructure,
        RoughStoryboard, PHASE2_POOL_SIZE, PHASE3_MAX_ALTERNATES,
    };
    use crate::models::{StoryboardBeat, StoryboardContent, StoryboardShot, StoryboardSource};
    use std::collections::{HashMap, HashSet};

    fn scored(source: StoryboardSource, total: f64) -> crate::storyboard::scoring::ScoredCandidate {
        crate::storyboard::scoring::ScoredCandidate {
            source,
            score: crate::storyboard::scoring::CandidateScore {
                total,
                semantic: 0.0,
                lexical: 0.0,
                clip: 0.0,
                quality: 0.0,
                duration: 0.0,
                freshness: 0.0,
                has_evidence: true,
                matched_keywords: vec![],
                shot_type: 0.0,
            },
        }
    }

    #[test]
    fn pick_segment_pool_skips_duplicate_asset_ids_and_fills_to_nine() {
        let ranked = (0..20)
            .map(|index| {
                let id = if index % 3 == 0 {
                    "dup".to_owned()
                } else {
                    format!("asset-{index}")
                };
                scored(source(&id), (20 - index) as f64)
            })
            .collect::<Vec<_>>();
        let (pool, scores, exhausted) = pick_segment_pool(&ranked, PHASE2_POOL_SIZE, 9);
        assert!(!exhausted);
        assert_eq!(pool.len(), 9);
        assert_eq!(scores.len(), 9);
        let unique = pool
            .iter()
            .map(|item| item.asset_id.as_str())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(unique.len(), 9);
        assert!(unique.contains("dup"));
    }

    #[test]
    fn pick_segment_pool_allows_two_non_overlapping_segments_of_one_asset() {
        let mut ranked = vec![
            scored(source_segment("multi", "s001", 0, 2_000), 30.0),
            scored(source_segment("multi", "s008", 8_000, 10_000), 29.0),
        ];
        ranked.extend(
            (0..10).map(|index| scored(source(&format!("asset-{index}")), (20 - index) as f64)),
        );
        let (pool, _, exhausted) = pick_segment_pool(&ranked, PHASE2_POOL_SIZE, 9);
        assert!(!exhausted);
        assert_eq!(pool.len(), 9);
        let multi = pool.iter().filter(|item| item.asset_id == "multi").count();
        assert_eq!(multi, 2);
    }

    #[test]
    fn pick_segment_pool_keeps_two_lookalikes_and_skips_the_rest() {
        let ranked = (0..20)
            .map(|index| {
                let mut item = source(&format!("asset-{index}"));
                item.visual_evidence = vec![crate::models::VisualEvidence {
                    time_ms: Some(0),
                    subjects: if index < 12 {
                        vec!["same-cabinet".to_owned()]
                    } else {
                        vec![format!("unique-{index}")]
                    },
                    scene: None,
                    actions: vec![],
                    products: vec![],
                    quality_notes: vec![],
                    shot_type: None,
                    camera_motion: None,
                    segment_id: None,
                    narrative_role: None,
                    caption: None,
                }];
                scored(item, (20 - index) as f64)
            })
            .collect::<Vec<_>>();
        let (pool, _, exhausted) = pick_segment_pool(&ranked, PHASE2_POOL_SIZE, 9);
        assert!(!exhausted);
        let ids: Vec<_> = pool.iter().map(|item| item.asset_id.as_str()).collect();
        assert_eq!(ids.len(), 9);
        assert_eq!(ids[0], "asset-0");
        assert_eq!(ids[1], "asset-1");
        assert!(!ids.contains(&"asset-2"));
        assert!(ids.contains(&"asset-12"));
        let lookalikes = ids
            .iter()
            .filter(|id| {
                id.trim_start_matches("asset-")
                    .parse::<usize>()
                    .is_ok_and(|index| index < 12)
            })
            .count();
        assert_eq!(lookalikes, 2);
    }

    #[test]
    fn pick_segment_pool_includes_component_leaders_after_score_first_slots() {
        let mut ranked = (0..12)
            .map(|index| scored(source(&format!("asset-{index}")), (30 - index) as f64))
            .collect::<Vec<_>>();
        ranked[9].score.clip = 25.0;
        ranked[10].score.semantic = 25.0;
        ranked[11].score.lexical = 25.0;
        let (pool, _, exhausted) = pick_segment_pool(&ranked, PHASE2_POOL_SIZE, 5);
        let ids = pool
            .iter()
            .map(|item| item.asset_id.as_str())
            .collect::<Vec<_>>();
        assert!(!exhausted);
        assert_eq!(ids.len(), 9);
        assert_eq!(
            &ids[..5],
            &["asset-0", "asset-1", "asset-2", "asset-3", "asset-4"]
        );
        assert!(ids.contains(&"asset-9"));
        assert!(ids.contains(&"asset-10"));
        assert!(ids.contains(&"asset-11"));
    }

    #[test]
    fn phase2_covers_a_beat_with_one_segment() {
        let narrative = NarrativeStructure {
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 4_000,
            script_mode: "key_message".to_owned(),
            shot_length_hint: String::new(),
            spoken_script: String::new(),
            beats: vec![beat()],
        };
        let rough = phase2_rough_shot_selection(
            &narrative,
            &[source("only")],
            &HashMap::new(),
            &[],
            &[],
            crate::storyboard::timing::SpeechTiming::default(),
            9,
        )
        .expect("one segment should cover the beat");
        assert!(rough.uncovered_beat_ids.is_empty());
        assert_eq!(rough.candidate_pools.len(), 1);
        assert_eq!(rough.candidate_pools[0].candidates.len(), 1);
        assert_eq!(rough.shots.len(), 1);
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
            segment_id: None,
        }
    }

    fn shot_segment(
        asset_id: &str,
        segment_id: &str,
        start_ms: i64,
        end_ms: i64,
    ) -> StoryboardShot {
        let mut item = shot(asset_id);
        item.segment_id = Some(segment_id.to_owned());
        item.source_start_ms = start_ms;
        item.source_end_ms = end_ms;
        item.duration_ms = (end_ms - start_ms).max(1);
        item
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
            segment: None,
            segment_embedding: None,
            segment_clip_embedding: None,
        }
    }

    fn source_segment(
        asset_id: &str,
        segment_id: &str,
        start_ms: i64,
        end_ms: i64,
    ) -> StoryboardSource {
        let mut item = source(asset_id);
        item.segment = Some(crate::models::CandidateSegment {
            id: segment_id.to_owned(),
            start_ms,
            end_ms,
            frame_paths: vec![],
            shot_type: None,
            camera_motion: None,
        });
        item
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
    fn library_inventory_summary_prefers_frequent_visual_tags() {
        let mut certificate = source("cert");
        certificate.visual_evidence = vec![crate::models::VisualEvidence {
            time_ms: Some(0),
            subjects: vec!["framed certificates".to_owned(), "presenter".to_owned()],
            scene: Some("certificate wall display".to_owned()),
            actions: vec!["pointing at plaques".to_owned()],
            products: vec![],
            quality_notes: vec![],
            shot_type: None,
            camera_motion: None,
            segment_id: None,
            narrative_role: None,
            caption: None,
        }];
        let mut factory = source("factory");
        factory.visual_evidence = vec![crate::models::VisualEvidence {
            time_ms: Some(0),
            subjects: vec!["battery modules".to_owned()],
            scene: Some("factory production line".to_owned()),
            actions: vec!["assembling cells".to_owned()],
            products: vec!["battery modules".to_owned()],
            quality_notes: vec![],
            shot_type: None,
            camera_motion: None,
            segment_id: None,
            narrative_role: None,
            caption: None,
        }];
        let summary = super::build_library_inventory_summary(&[certificate, factory]);
        assert!(summary.contains("framed certificates"));
        assert!(summary.contains("certificate wall display"));
        assert!(summary.contains("battery modules"));
        assert!(summary.contains("withVisualEvidence=2"));
        assert!(summary.chars().count() <= super::INVENTORY_MAX_CHARS);
    }

    #[test]
    fn library_inventory_summary_handles_empty_evidence() {
        let summary = super::build_library_inventory_summary(&[source("bare")]);
        assert!(summary.contains("tags=(none yet"));
        assert!(summary.contains("videos=1"));
    }

    #[test]
    fn candidate_indexes_resolve_whole_assets_and_exact_segments() {
        let mut pool = candidate_pool("beat-1", &["whole", "segmented", "segmented"]);
        // 整素材也带场景描述，但其编号不能被误当作选中的片段。
        pool.candidates[0].scene_segments = vec![serde_json::from_value(serde_json::json!({
            "id":"s001", "startMs":0, "endMs":10000
        }))
        .unwrap()];
        for (index, start) in [(1, 2000), (2, 5000)] {
            pool.candidates[index].segment = Some(crate::models::CandidateSegment {
                id: format!("s00{index}"),
                start_ms: start,
                end_ms: start + 2000,
                frame_paths: vec![],
                shot_type: None,
                camera_motion: None,
            });
        }
        let rough = RoughStoryboard {
            speech_timing: Default::default(),
            title: "test".to_owned(),
            summary: String::new(),
            target_duration_ms: 6000,
            script_mode: "key_message".to_owned(),
            shot_length_hint: String::new(),
            beats: vec![beat()],
            uncovered_beat_ids: vec![],
            shots: vec![shot("whole")],
            candidate_pools: vec![pool],
        };
        let mut selected = super::assemble_phase3_selection(
            "test",
            &rough,
            r#"{"selections":[{"beatId":"beat-1","candidateIndexes":[0,2],"uncovered":false}]}"#,
        )
        .unwrap();
        assert_eq!(selected.shots[0].asset_id, "whole");
        assert_eq!(selected.shots[0].segment_id, None);
        assert_eq!(selected.shots[0].source_start_ms, 0);
        assert_eq!(selected.shots[0].source_end_ms, 10_000);
        assert_eq!(selected.shots[1].segment_id.as_deref(), Some("s002"));
        assert_eq!(selected.shots[1].source_start_ms, 5000);
        assert_eq!(selected.shots[1].source_end_ms, 7000);
        assert!(collect_phase3_issues(&mut selected, &rough).is_empty());
        let invalid = super::assemble_phase3_selection(
            "test",
            &rough,
            r#"{"selections":[{"beatId":"beat-1","candidateIndexes":[99],"uncovered":false}]}"#,
        )
        .err()
        .expect("out-of-pool index must fail");
        assert!(invalid.contains("outside beat"));
    }

    #[test]
    fn phase3_can_select_the_ninth_candidate() {
        let rough = RoughStoryboard {
            speech_timing: Default::default(),
            title: "test".to_owned(),
            summary: String::new(),
            target_duration_ms: 3_000,
            script_mode: "key_message".to_owned(),
            shot_length_hint: String::new(),
            beats: vec![beat()],
            uncovered_beat_ids: vec![],
            shots: vec![shot("asset-0")],
            candidate_pools: vec![candidate_pool(
                "beat-1",
                &[
                    "asset-0", "asset-1", "asset-2", "asset-3", "asset-4", "asset-5", "asset-6",
                    "asset-7", "asset-8",
                ],
            )],
        };
        let selected = super::assemble_phase3_selection(
            "test",
            &rough,
            r#"{"selections":[{"beatId":"beat-1","candidateIndexes":[8],"uncovered":false}]}"#,
        )
        .unwrap();
        assert_eq!(selected.shots[0].asset_id, "asset-8");
    }

    #[test]
    fn phase2_sends_nine_segment_candidates_to_the_model() {
        assert_eq!(PHASE2_POOL_SIZE, 9);
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
            shot_length_hint: String::new(),
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
            shot_length_hint: String::new(),
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
            shot_length_hint: String::new(),
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
    fn phase3_allows_one_shot_when_pool_has_alternates() {
        let rough_shot = shot("selected");
        let rough = RoughStoryboard {
            speech_timing: Default::default(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 1_000,
            script_mode: "key_message".to_owned(),
            shot_length_hint: String::new(),
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
                .all(|issue| issue.kind != "beat_below_min_shots"
                    && issue.kind != "beat_below_min_shots_insufficient_pool"),
            "one honest shot is enough even when the pool has alternates; issues={issues:?}"
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
            shot_length_hint: String::new(),
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
    fn phase3_rejects_overlapping_same_asset_across_beats() {
        // 跨 beat 相邻同片且源范围重叠仍拒绝；不同非相似段见下一测。
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
            shot_length_hint: String::new(),
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
            shots: vec![shot_a, shot_b, shot_c, shot_d],
        };
        let issues = collect_phase3_issues(&mut final_content, &rough);
        assert!(
            issues
                .iter()
                .any(|issue| issue.kind == "similar_used_segment"),
            "overlapping same asset across beats must be rejected; issues={issues:?}"
        );
    }

    #[test]
    fn phase3_allows_adjacent_different_segments_of_same_asset() {
        let mut beat_one = beat();
        beat_one.id = "beat-1".to_owned();
        let mut beat_two = beat();
        beat_two.id = "beat-2".to_owned();
        let mut shot_a = shot("alt-a");
        shot_a.order_index = 1;
        shot_a.beat_id = "beat-1".to_owned();
        let mut shot_b = shot_segment("shared", "s001", 0, 2_000);
        shot_b.order_index = 2;
        shot_b.beat_id = "beat-1".to_owned();
        let mut shot_c = shot_segment("shared", "s008", 8_000, 10_000);
        shot_c.order_index = 3;
        shot_c.beat_id = "beat-2".to_owned();
        let mut shot_d = shot("alt-b");
        shot_d.order_index = 4;
        shot_d.beat_id = "beat-2".to_owned();
        let mut beat_three = beat();
        beat_three.id = "beat-3".to_owned();
        let mut shot_e = shot("alt-c");
        shot_e.order_index = 5;
        shot_e.beat_id = "beat-3".to_owned();
        let mut shot_f = shot("alt-d");
        shot_f.order_index = 6;
        shot_f.beat_id = "beat-3".to_owned();
        let mut pool_one = candidate_pool("beat-1", &["shared", "alt-a", "alt-x"]);
        pool_one.candidates[0] = source_segment("shared", "s001", 0, 2_000);
        let mut pool_two = candidate_pool("beat-2", &["shared", "alt-b", "alt-y"]);
        pool_two.candidates[0] = source_segment("shared", "s008", 8_000, 10_000);
        let pool_three = candidate_pool("beat-3", &["alt-c", "alt-d", "alt-z"]);
        let rough = RoughStoryboard {
            speech_timing: Default::default(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 12_000,
            script_mode: "key_message".to_owned(),
            shot_length_hint: String::new(),
            beats: vec![beat_one.clone(), beat_two.clone(), beat_three.clone()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![shot_b.clone(), shot_c.clone(), shot_e.clone()],
            candidate_pools: vec![pool_one, pool_two, pool_three],
        };
        let mut final_content = StoryboardContent {
            brief: String::new(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 12_000,
            script_mode: "key_message".to_owned(),
            beats: vec![beat_one, beat_two, beat_three],
            uncovered_beat_ids: Vec::new(),
            shots: vec![shot_a, shot_b, shot_c, shot_d, shot_e, shot_f],
        };
        let issues = collect_phase3_issues(&mut final_content, &rough);
        assert!(
            issues.is_empty(),
            "different non-overlapping segments of the same asset may sit adjacent; issues={issues:?}"
        );
    }

    #[test]
    fn phase3_rejects_visually_similar_used_segments() {
        let mut beat_one = beat();
        beat_one.id = "beat-1".to_owned();
        let mut beat_two = beat();
        beat_two.id = "beat-2".to_owned();
        let mut shot_a = shot("alt-a");
        shot_a.order_index = 1;
        shot_a.beat_id = "beat-1".to_owned();
        let mut shot_b = shot_segment("look-a", "s001", 0, 2_000);
        shot_b.order_index = 2;
        shot_b.beat_id = "beat-1".to_owned();
        let mut shot_c = shot_segment("look-b", "s002", 0, 2_000);
        shot_c.order_index = 3;
        shot_c.beat_id = "beat-2".to_owned();
        let mut shot_d = shot("alt-b");
        shot_d.order_index = 4;
        shot_d.beat_id = "beat-2".to_owned();
        let mut look_a = source_segment("look-a", "s001", 0, 2_000);
        look_a.visual_evidence = vec![crate::models::VisualEvidence {
            time_ms: Some(0),
            subjects: vec!["cabinet-interior".to_owned()],
            scene: Some("factory cabinet".to_owned()),
            actions: vec!["presenting".to_owned()],
            products: vec![],
            quality_notes: vec![],
            shot_type: None,
            camera_motion: None,
            segment_id: Some("s001".to_owned()),
            narrative_role: None,
            caption: None,
        }];
        let mut look_b = source_segment("look-b", "s002", 0, 2_000);
        look_b.visual_evidence = vec![look_a.visual_evidence[0].clone()];
        let mut pool_one = candidate_pool("beat-1", &["look-a", "alt-a", "alt-c"]);
        pool_one.candidates[0] = look_a;
        let mut pool_two = candidate_pool("beat-2", &["look-b", "alt-b", "alt-d"]);
        pool_two.candidates[0] = look_b;
        let rough = RoughStoryboard {
            speech_timing: Default::default(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 8_000,
            script_mode: "key_message".to_owned(),
            shot_length_hint: String::new(),
            beats: vec![beat_one.clone(), beat_two.clone()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![shot_b.clone(), shot_c.clone()],
            candidate_pools: vec![pool_one, pool_two],
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
                .any(|issue| issue.kind == "similar_used_segment"),
            "similar segments already used must be rejected; issues={issues:?}"
        );
    }

    #[test]
    fn phase3_rejects_asset_over_diversity_limit_even_when_non_adjacent() {
        // 4 镜上限为 1：非相邻复用同一 asset 的不同段也必须在 Phase 3 拦下。
        let mut beat_one = beat();
        beat_one.id = "beat-1".to_owned();
        let mut beat_two = beat();
        beat_two.id = "beat-2".to_owned();
        let mut shot_a = shot_segment("shared", "s001", 0, 2_000);
        shot_a.order_index = 1;
        shot_a.beat_id = "beat-1".to_owned();
        let mut shot_b = shot("alt-a");
        shot_b.order_index = 2;
        shot_b.beat_id = "beat-1".to_owned();
        let mut shot_c = shot("alt-b");
        shot_c.order_index = 3;
        shot_c.beat_id = "beat-2".to_owned();
        let mut shot_d = shot_segment("shared", "s008", 8_000, 10_000);
        shot_d.order_index = 4;
        shot_d.beat_id = "beat-2".to_owned();
        let mut pool_one = candidate_pool("beat-1", &["shared", "alt-a", "alt-c"]);
        pool_one.candidates[0] = source_segment("shared", "s001", 0, 2_000);
        let mut pool_two = candidate_pool("beat-2", &["shared", "alt-b", "alt-d"]);
        pool_two.candidates[0] = source_segment("shared", "s008", 8_000, 10_000);
        let rough = RoughStoryboard {
            speech_timing: Default::default(),
            title: "title".to_owned(),
            summary: "summary".to_owned(),
            target_duration_ms: 8_000,
            script_mode: "key_message".to_owned(),
            shot_length_hint: String::new(),
            beats: vec![beat_one.clone(), beat_two.clone()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![shot_a.clone(), shot_c.clone()],
            candidate_pools: vec![pool_one, pool_two],
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
                .any(|issue| issue.kind == "similar_used_segment"),
            "fixture must remain non-adjacent and non-similar; issues={issues:?}"
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
            shot_length_hint: String::new(),
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
            shot_length_hint: String::new(),
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
            shot_length_hint: String::new(),
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
    fn narration_estimate_follows_language_and_target() {
        use crate::storyboard::multimodal::Phase4ContentWindow;
        use std::collections::HashMap;

        let chinese = "这是一句比较完整的口播文案需要足够画面";
        let chinese_ms = super::estimated_narration_ms(chinese);
        let old_two_char_units = ((chinese.chars().count() as f64) / 2.0).ceil() * 300.0;
        assert!(
            (chinese_ms as f64) < old_two_char_units,
            "chinese estimate {chinese_ms} should be faster than the old two-character clock"
        );
        assert_eq!(super::estimated_narration_ms("hello world"), 600);

        let mut long = shot("selected");
        long.order_index = 1;
        long.narration_text = chinese.to_owned();
        long.duration_ms = 8_000;
        long.source_start_ms = 0;
        long.source_end_ms = 8_000;
        let mut short = shot("other");
        short.order_index = 2;
        short.beat_id = "beat-2".to_owned();
        short.narration_text = "Hi".to_owned();
        short.duration_ms = 8_000;
        short.source_start_ms = 0;
        short.source_end_ms = 8_000;
        let mut content = StoryboardContent {
            brief: String::new(),
            title: "t".to_owned(),
            summary: "s".to_owned(),
            target_duration_ms: 10_000,
            script_mode: "full_script".to_owned(),
            beats: vec![beat()],
            uncovered_beat_ids: Vec::new(),
            shots: vec![long, short],
        };
        let mut pick_map = HashMap::new();
        for (order, asset) in [(1_i64, "selected"), (2, "other")] {
            pick_map.insert(
                order,
                (
                    Phase4ContentWindow {
                        window_id: format!("{asset}:w0"),
                        asset_id: asset.to_owned(),
                        start_ms: 0,
                        end_ms: 20_000,
                    },
                    false,
                ),
            );
        }
        apply_narration_phrase_duration_floor(&mut content, &pick_map);
        let total = content.shots.iter().map(|shot| shot.duration_ms).sum::<i64>();
        assert_eq!(total, 10_000);
        assert!(content.shots[0].duration_ms > content.shots[1].duration_ms);
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
    fn resolve_overlaps_scoped_leaves_frozen_shots_unchanged() {
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
                    item.source_start_ms = 0;
                    item.source_end_ms = 4_000;
                    item.duration_ms = 4_000;
                    item
                },
                {
                    let mut item = shot("shared");
                    item.order_index = 2;
                    item.beat_id = "beat-2".to_owned();
                    item.crop_focus = Some([0.6, 0.5]);
                    item.source_start_ms = 2_000;
                    item.source_end_ms = 6_000;
                    item.duration_ms = 4_000;
                    item
                },
            ],
        };
        let window = Phase4ContentWindow {
            window_id: "shared:w0".to_owned(),
            asset_id: "shared".to_owned(),
            start_ms: 0,
            end_ms: 12_000,
        };
        let mut pick_map = HashMap::new();
        pick_map.insert(1, (window.clone(), false));
        pick_map.insert(2, (window, false));
        let mutable = HashSet::from([2]);
        resolve_overlaps_within_chosen_windows_scoped(&mut content, &pick_map, Some(&mutable));
        assert_eq!(content.shots[0].crop_focus, Some([0.4, 0.5]));
        assert_eq!(content.shots[0].source_start_ms, 0);
        assert_eq!(content.shots[0].source_end_ms, 4_000);
        assert!(
            content.shots[1].source_start_ms >= 4_000
                || content.shots[1].source_end_ms <= content.shots[0].source_start_ms
        );
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
            shot_length_hint: String::new(),
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
            shot_length_hint: String::new(),
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
        let cards = phase3_pool_cards(&[pool], &[], &HashSet::new(), &Default::default());
        assert_eq!(cards.len(), 1);
        assert_eq!(
            cards[0]["candidates"].as_array().map(|items| items.len()),
            Some(12)
        );
        assert_eq!(cards[0]["candidates"][0]["keyframeGridAttached"], false);
        assert_eq!(cards[0]["requiredVisual"], "");
    }

    #[test]
    fn phase3_pool_cards_include_visible_caption() {
        let mut candidate = source("a0");
        candidate.visual_evidence = vec![crate::models::VisualEvidence {
            time_ms: Some(0),
            subjects: vec!["packed units".to_owned()],
            scene: Some("warehouse aisle".to_owned()),
            actions: vec!["stacked".to_owned()],
            products: vec![],
            quality_notes: vec![],
            shot_type: None,
            camera_motion: None,
            segment_id: None,
            narrative_role: Some("establishing stored goods".to_owned()),
            caption: Some("Rows of wrapped packs with a shipping overlay.".to_owned()),
        }];
        let pool = BeatCandidatePool {
            beat_id: "beat-1".to_owned(),
            beat_purpose: "purpose".to_owned(),
            candidates: vec![candidate],
            scores: vec![],
        };
        let cards = phase3_pool_cards(&[pool], &[], &HashSet::new(), &Default::default());
        assert_eq!(
            cards[0]["candidates"][0]["visibleCaption"],
            "Rows of wrapped packs with a shipping overlay."
        );
        assert_eq!(cards[0]["candidates"][0]["scene"], "warehouse aisle");
    }
}

/// Phase 3: 每个 beat 单独看最多 9 张候选图，默认选 1 镜；池内候选均可选。
///
/// 返回 `(候选, 校验问题)`：`Err` 仅表示传输/解析失败；语义问题进 `issues`。
pub(crate) fn phase3_select(
    app: &AppHandle,
    access: &ModelAccess,
    brief: &str,
    rough: &RoughStoryboard,
    repair: Option<&RepairPacket>,
) -> Result<(StoryboardContent, Vec<StoryboardIssue>), String> {
    log::info!(
        "Phase 3: Selecting one shot per beat from {} pools (9 images each)",
        rough.candidate_pools.len()
    );
    let retry_beats = beats_named_in_repair(repair, rough);
    let previous = previous_beat_selections(repair, rough);
    let mut selections = Vec::new();
    let mut chosen_so_far = Vec::new();
    for beat_id in covered_beat_ids(rough) {
        if !retry_beats.contains(&beat_id) {
            if let Some(kept) = previous.get(&beat_id).cloned() {
                chosen_so_far.extend(refs_from_selection(rough, &kept));
                selections.push(kept);
                continue;
            }
        }
        let Some(pool) = rough
            .candidate_pools
            .iter()
            .find(|pool| pool.beat_id == beat_id)
        else {
            selections.push(Phase3BeatSelection {
                beat_id: beat_id.clone(),
                candidate_indexes: Vec::new(),
                uncovered: true,
                narration: None,
                match_level: None,
            });
            continue;
        };
        if pool.candidates.is_empty() {
            log::info!("Phase 3 beat '{beat_id}': empty pool, uncovered");
            selections.push(Phase3BeatSelection {
                beat_id: beat_id.clone(),
                candidate_indexes: Vec::new(),
                uncovered: true,
                narration: None,
                match_level: None,
            });
            continue;
        }
        let mut last_error = None;
        let mut picked = None;
        for attempt in 1..=3 {
            crate::execution_deadline::check()?;
            match select_one_beat(
                app,
                access,
                brief,
                rough,
                pool,
                &chosen_so_far,
                repair,
                attempt,
            ) {
                Ok(selection) => {
                    if selection
                        .candidate_indexes
                        .iter()
                        .any(|index| *index >= pool.candidates.len())
                    {
                        last_error = Some(format!(
                            "Phase 3 candidateIndex is outside beat '{beat_id}' candidate pool."
                        ));
                        continue;
                    }
                    picked = Some(selection);
                    last_error = None;
                    break;
                }
                Err(error) => {
                    log::warn!("Phase 3 beat '{beat_id}' attempt {attempt} failed: {error}");
                    last_error = Some(error);
                }
            }
        }
        let selection = picked.unwrap_or_else(|| Phase3BeatSelection {
            beat_id: beat_id.clone(),
            candidate_indexes: Vec::new(),
            uncovered: true,
            narration: None,
            match_level: None,
        });
        if last_error.is_some() {
            log::warn!("Phase 3 beat '{beat_id}' exhausted local retries; leaving uncovered");
        }
        chosen_so_far.extend(refs_from_selection(rough, &selection));
        selections.push(selection);
    }
    let payload = json!({ "selections": selections });
    let text = payload.to_string();
    let mut selected = assemble_phase3_selection(brief, rough, &text)?;
    let mut issues = collect_phase3_issues(&mut selected, rough);
    if selected.shots.is_empty() && !covered_beat_ids(rough).is_empty() {
        issues.push(
            StoryboardIssue::new(
                "insufficient_footage",
                "No playable shot could be selected from the ready library. Ask the user how to continue.",
                false,
            )
            .allowing(vec!["ask the user whether to import more footage, skip beats, or change duration"]),
        );
    }
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

fn beats_named_in_repair(
    repair: Option<&RepairPacket>,
    rough: &RoughStoryboard,
) -> HashSet<String> {
    let Some(repair) = repair else {
        return covered_beat_ids(rough).into_iter().collect();
    };
    let mut beats = HashSet::new();
    for shot in &repair.previous_shots {
        if repair
            .issues
            .iter()
            .any(|issue| issue.affected_shots.contains(&shot.shot_index))
        {
            beats.insert(shot.beat_id.clone());
        }
    }
    for issue in &repair.issues {
        if issue.kind == "insufficient_footage" || issue.kind.contains("uncovered") {
            beats.extend(covered_beat_ids(rough));
            break;
        }
        if let Some(beat_id) = issue.message.split('\'').nth(1) {
            beats.insert(beat_id.to_owned());
        }
    }
    if beats.is_empty() {
        covered_beat_ids(rough).into_iter().collect()
    } else {
        beats
    }
}

fn previous_beat_selections(
    repair: Option<&RepairPacket>,
    rough: &RoughStoryboard,
) -> HashMap<String, Phase3BeatSelection> {
    let Some(repair) = repair else {
        return HashMap::new();
    };
    let mut by_beat: HashMap<String, Vec<usize>> = HashMap::new();
    for shot in &repair.previous_shots {
        let Some(pool) = rough
            .candidate_pools
            .iter()
            .find(|pool| pool.beat_id == shot.beat_id)
        else {
            continue;
        };
        let Some(index) = pool.candidates.iter().position(|candidate| {
            candidate.asset_id == shot.asset_id
                && candidate
                    .segment
                    .as_ref()
                    .map(|segment| segment.start_ms == shot.source_start_ms)
                    .unwrap_or(true)
        }) else {
            continue;
        };
        by_beat.entry(shot.beat_id.clone()).or_default().push(index);
    }
    by_beat
        .into_iter()
        .map(|(beat_id, candidate_indexes)| {
            (
                beat_id.clone(),
                Phase3BeatSelection {
                    beat_id,
                    uncovered: candidate_indexes.is_empty(),
                    candidate_indexes,
                    narration: None,
                    match_level: None,
                },
            )
        })
        .collect()
}

fn refs_from_selection(
    rough: &RoughStoryboard,
    selection: &Phase3BeatSelection,
) -> Vec<(String, String, String)> {
    let Some(pool) = rough
        .candidate_pools
        .iter()
        .find(|pool| pool.beat_id == selection.beat_id)
    else {
        return Vec::new();
    };
    selection
        .candidate_indexes
        .iter()
        .filter_map(|index| {
            let candidate = pool.candidates.get(*index)?;
            Some((
                selection.beat_id.clone(),
                candidate.asset_id.clone(),
                candidate
                    .segment
                    .as_ref()
                    .map(|segment| segment.id.clone())
                    .unwrap_or_else(|| "-".to_owned()),
            ))
        })
        .collect()
}

fn select_one_beat(
    app: &AppHandle,
    access: &ModelAccess,
    brief: &str,
    rough: &RoughStoryboard,
    pool: &BeatCandidatePool,
    already_selected: &[(String, String, String)],
    repair: Option<&RepairPacket>,
    attempt: usize,
) -> Result<Phase3BeatSelection, String> {
    let (keyframe_blocks, attached_indexes) = phase3_keyframe_image_blocks_for_pool(app, pool);
    let candidate_cards_json = serde_json::to_string(&phase3_pool_cards(
        std::slice::from_ref(pool),
        &rough.beats,
        &attached_indexes,
        &rough.speech_timing,
    ))
    .unwrap_or_else(|_| "[]".to_owned());
    let feedback_context = repair
        .filter(|packet| {
            packet.issues.iter().any(|issue| {
                issue.message.contains(&pool.beat_id)
                    || packet.previous_shots.iter().any(|shot| {
                        shot.beat_id == pool.beat_id
                            && issue.affected_shots.contains(&shot.shot_index)
                    })
            })
        })
        .map(repair_packet_prompt_block)
        .unwrap_or_default();
    let already = already_selected
        .iter()
        .map(|(beat_id, asset_id, segment_id)| format!("{beat_id}:{asset_id}:{segment_id}"))
        .collect::<Vec<_>>()
        .join(", ");
    let pacing_line =
        normalize_shot_length_hint(&rough.shot_length_hint).prompt_line();
    let prompt = format!(
        "Brief: {brief}\n\
        Narrative title/summary/target: {} / {} / {}ms\n\
        Script mode: {}\n\
        This request is for ONE beat only: {}\n\
        Already selected shots (assetId:segmentId): {}\n\
        Uncovered beat ids (do not create shots for these): {}\n\
        Beat timing plan: {}\n\
        Candidate pool for this beat: {candidate_cards_json}\n\
        {feedback_context}\n\n\
        Keyframe grids for this beat's candidates are attached below (up to 9). Each image is this candidate's own time window, not a stand-in for the whole file.\n\
        Candidates with keyframeGridAttached=false have no image — judge them from visibleCaption, scene, subjects, and visualTags.\n\
        Each candidate lists usableMs: the playable motion window, not the whole hard-cut span.\n\
        If narrationMs is longer than the selected candidate's usableMs, keep the best visual match. The program will slow that clip to cover the spoken duration. Do not swap only to get more usableMs, and do not add a second shot.\n\
        Selection order: (1) look at the attached frames first; visibleCaption/scene/subjects/actions are unverified labels — if they conflict with the frames, trust the frames; (2) prefer the candidate whose frames best cover this beat's requiredVisual; (3) if none cover it literally, pick the closest honest scene-setting clip from this pool.\n\
        Narration is what will be spoken and how long the picture must last. Do not pick a clip only because it echoes abstract wording in the narration or brief.\n\
        Do not invent camera motion or events absent from the frames. Do not treat a caption as true when the grid shows something else.\n\
        Hard rule: candidateIndexes must be DISTINCT assetIds from THIS beat's pool. Every listed candidateIndex is selectable.\n\
        Hard rule: do not pick a candidate visually similar to an already selected shot.\n\
        Choose ONE candidate. A second distinct, non-similar asset is allowed only when two clips honestly cover different parts of requiredVisual; never pad.\n\
        {pacing_line}\n\
        matchLevel must be 'direct' when the chosen frames visibly cover requiredVisual, otherwise 'contextual'.\n\
        Return JSON only: {{\"selections\":[{{\"beatId\":\"{}\",\"candidateIndexes\":[0],\"uncovered\":false,\"matchLevel\":\"contextual\",\"narration\":null}}]}}\n\
        Include exactly one selection object for this beat. Omit narration unless moving words to a neighbor beat.",
        rough.title,
        rough.summary,
        rough.target_duration_ms,
        rough.script_mode,
        pool.beat_id,
        already,
        rough.uncovered_beat_ids.join(", "),
        serde_json::to_string(&rough.speech_timing).unwrap(),
        pool.beat_id
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
        Some(&pool.beat_id),
        attempt,
        "request",
        &request,
    );
    let body = post_model_payload(access, &request, Some(STORYBOARD_TIMEOUT))?;
    let response_value =
        serde_json::from_str::<Value>(&body).unwrap_or_else(|_| json!({ "raw": body }));
    crate::storyboard::provider_trace::append_storyboard_trace(
        "Phase 3",
        Some(&pool.beat_id),
        attempt,
        "response",
        &response_value,
    );
    let text = model_response_json_text(access, &body)
        .ok_or_else(|| "Phase 3 response did not contain JSON.".to_owned())?;
    let parsed: Phase3SelectResponse = serde_json::from_str(&text)
        .map_err(|_| "Phase 3 JSON did not match selection schema.".to_owned())?;
    let selection = parsed
        .selections
        .into_iter()
        .find(|item| item.beat_id == pool.beat_id)
        .ok_or_else(|| format!("Phase 3 selection missing covered beat '{}'.", pool.beat_id))?;
    let grid_attached = attached_indexes;
    let selected = selection
        .candidate_indexes
        .iter()
        .filter_map(|index| {
            let candidate = pool.candidates.get(*index)?;
            Some(json!({
                "candidateIndex": index,
                "assetId": candidate.asset_id,
                "segmentId": candidate.segment.as_ref().map(|segment| segment.id.clone()),
                "keyframeGridAttached": grid_attached.contains(index),
                "matchLevel": normalize_phase3_match_level(selection.match_level.as_deref()),
                "evidence": candidate_evidence_summary(candidate),
            }))
        })
        .collect::<Vec<_>>();
    crate::storyboard::provider_trace::append_pool_trace(
        "Phase 3",
        &pool.beat_id,
        &json!({
            "attempt": attempt,
            "uncovered": selection.uncovered,
            "candidateIndexes": selection.candidate_indexes,
            "selected": selected,
            "attachedGridCount": grid_attached.len(),
            "pool": pool_trace_candidates(pool.candidates.as_slice(), pool.scores.as_slice()),
        }),
    );
    Ok(selection)
}

fn pool_trace_candidates(
    pool: &[StoryboardSource],
    scores: &[scoring::CandidateScore],
) -> Vec<Value> {
    pool.iter()
        .enumerate()
        .map(|(index, candidate)| {
            let score = scores.get(index);
            json!({
                "candidateIndex": index,
                "assetId": candidate.asset_id,
                "segmentId": candidate.segment.as_ref().map(|segment| segment.id.clone()),
                "rangeMs": candidate.segment.as_ref().map(|segment| {
                    json!([segment.start_ms, segment.end_ms])
                }),
                "hasKeyframeGrid": candidate.keyframe_grid_path.as_deref().is_some_and(|path| Path::new(path).is_file()),
                "evidence": candidate_evidence_summary(candidate),
                "score": score.map(|score| json!({
                    "total": (score.total * 10.0).round() / 10.0,
                    "semantic": (score.semantic * 10.0).round() / 10.0,
                    "lexical": (score.lexical * 10.0).round() / 10.0,
                    "clip": (score.clip * 10.0).round() / 10.0,
                    "quality": (score.quality * 10.0).round() / 10.0,
                    "duration": (score.duration * 10.0).round() / 10.0,
                    "freshness": (score.freshness * 10.0).round() / 10.0,
                    "hasEvidence": score.has_evidence,
                    "matchedKeywords": score.matched_keywords,
                })),
            })
        })
        .collect()
}

fn candidate_evidence_summary(candidate: &StoryboardSource) -> Value {
    let evidence = candidate
        .segment
        .as_ref()
        .and_then(|segment| {
            candidate
                .visual_evidence
                .iter()
                .find(|item| item.segment_id.as_deref() == Some(segment.id.as_str()))
                .or_else(|| candidate.visual_evidence.first())
        })
        .or_else(|| candidate.visual_evidence.first());
    match evidence {
        Some(evidence) => json!({
            "scene": evidence.scene,
            "caption": evidence.caption,
            "narrativeRole": evidence.narrative_role,
            "subjects": evidence.subjects.iter().take(4).cloned().collect::<Vec<_>>(),
            "actions": evidence.actions.iter().take(3).cloned().collect::<Vec<_>>(),
        }),
        None => json!({
            "scene": null,
            "caption": null,
            "narrativeRole": null,
            "subjects": [],
            "actions": [],
        }),
    }
}

/// 为这一拍附上最多 9 张候选网格：按该条时间窗现拼，缺文件则现抽。
fn phase3_keyframe_image_blocks_for_pool(
    app: &AppHandle,
    pool: &BeatCandidatePool,
) -> (Vec<Value>, HashSet<usize>) {
    use crate::storyboard::multimodal::{ensure_phase3_candidate_grid, read_input_image};

    let mut blocks = Vec::new();
    let mut attached = HashSet::new();
    let considered = pool
        .candidates
        .len()
        .min(crate::storyboard::multimodal::PHASE3_MAX_GRID_IMAGES);
    for (index, candidate) in pool.candidates.iter().take(considered).enumerate() {
        let Some(grid_path) = ensure_phase3_candidate_grid(Some(app), candidate) else {
            continue;
        };
        let Some(image) = read_input_image(&grid_path) else {
            continue;
        };
        attached.insert(index);
        blocks.push(json!({
            "type": "input_text",
            "text": format!("Keyframe grid (2x2) for candidateIndex={index} assetId={}", candidate.asset_id)
        }));
        blocks.push(image);
    }
    log::info!(
        "Phase 3 beat '{}' attached {}/{} candidate grid image(s)",
        pool.beat_id,
        attached.len(),
        considered
    );
    (blocks, attached)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Phase3SelectResponse {
    #[serde(default)]
    selections: Vec<Phase3BeatSelection>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Phase3BeatSelection {
    beat_id: String,
    candidate_indexes: Vec<usize>,
    #[serde(default)]
    uncovered: bool,
    #[serde(default)]
    narration: Option<String>,
    #[serde(default)]
    match_level: Option<String>,
}

fn normalize_phase3_match_level(value: Option<&str>) -> String {
    match value.map(|item| item.trim().to_ascii_lowercase()) {
        Some(level) if level == "direct" => "direct".to_owned(),
        _ => "contextual".to_owned(),
    }
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
    let (min_shot_ms, max_shot_ms) =
        normalize_shot_length_hint(&rough.shot_length_hint).duration_bounds();
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
        if selection.uncovered || selection.candidate_indexes.is_empty() {
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
        let part_count = selection.candidate_indexes.len() as i64;
        for (part_offset, index) in selection.candidate_indexes.iter().enumerate() {
            let candidate = pool.and_then(|pool| pool.candidates.get(*index)).ok_or_else(|| {
                format!("Phase 3 candidateIndex {index} is outside beat '{beat_id}' candidate pool.")
            })?;
            let duration =
                (per_beat_budget / part_count.max(1)).clamp(min_shot_ms, max_shot_ms);
            let (provisional_start, provisional_end) =
                if let Some(segment) = candidate.segment.as_ref() {
                    (segment.start_ms, segment.end_ms.max(segment.start_ms + 1))
                } else {
                    let asset_end = candidate.duration_ms.unwrap_or(duration).max(1);
                    (0, asset_end)
                };
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
                duration_ms: duration,
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
                asset_id: candidate.asset_id.clone(),
                source_start_ms: provisional_start,
                source_end_ms: provisional_end,
                reason: if candidate.segment.is_some() {
                    "Phase 3 selected segment; ranges pending Phase 4 refine.".to_owned()
                } else {
                    "Phase 3 selected whole asset; Phase 4 refines inside this window.".to_owned()
                },
                beat_id: beat_id.clone(),
                match_level: normalize_phase3_match_level(selection.match_level.as_deref()),
                beat_part_index: (part_offset as i64) + 1,
                beat_part_count: part_count,
                split_role: split_role.to_owned(),
                segment_id: candidate.segment.as_ref().map(|segment| segment.id.clone()),
            });
            order_index += 1;
        }
    }

    let mut beats = rough.beats.clone();
    for (beat_id, selection) in &by_beat {
        let Some(narration) = selection.narration.as_ref() else {
            continue;
        };
        if let Some(beat) = beats.iter_mut().find(|beat| beat.id == *beat_id) {
            beat.narration = narration.clone();
        }
    }

    Ok(StoryboardContent {
        brief: brief.to_owned(),
        title: rough.title.clone(),
        summary: rough.summary.clone(),
        target_duration_ms: rough.target_duration_ms,
        script_mode: rough.script_mode.clone(),
        beats,
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
    session: &mut crate::storyboard::phase4::Phase4Session,
) -> Result<(StoryboardContent, Vec<StoryboardIssue>), String> {
    crate::storyboard::phase4::phase4_refine_ranges(
        app, access, brief, selected, rough, sources, repair, session,
    )
}

#[cfg(test)]
fn clamp_shots_to_chosen_windows(
    content: &mut StoryboardContent,
    pick_map: &HashMap<i64, (crate::storyboard::multimodal::Phase4ContentWindow, bool)>,
) {
    clamp_shots_to_chosen_windows_scoped(content, pick_map, None);
}

pub(crate) fn clamp_shots_to_chosen_windows_scoped(
    content: &mut StoryboardContent,
    pick_map: &HashMap<i64, (crate::storyboard::multimodal::Phase4ContentWindow, bool)>,
    mutable: Option<&HashSet<i64>>,
) {
    for shot in &mut content.shots {
        if let Some(allowed) = mutable {
            if !allowed.contains(&shot.order_index) {
                continue;
            }
        }
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
        // 成片时长保持口播时钟；源窗被钳在可用区间内，短了就放慢。
    }
}

/// 同素材源范围交叠时，只在 Phase 4 已选内容窗内挪切点；挪过的清 cropFocus。
#[cfg(test)]
fn resolve_overlaps_within_chosen_windows(
    content: &mut StoryboardContent,
    pick_map: &HashMap<i64, (crate::storyboard::multimodal::Phase4ContentWindow, bool)>,
) {
    resolve_overlaps_within_chosen_windows_scoped(content, pick_map, None);
}

pub(crate) fn resolve_overlaps_within_chosen_windows_scoped(
    content: &mut StoryboardContent,
    pick_map: &HashMap<i64, (crate::storyboard::multimodal::Phase4ContentWindow, bool)>,
    mutable: Option<&HashSet<i64>>,
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
        let is_mutable = |order: i64| mutable.map_or(true, |allowed| allowed.contains(&order));
        for &index in &ordered {
            let shot = &content.shots[index];
            if !is_mutable(shot.order_index) {
                occupied.push((
                    shot.source_start_ms,
                    shot.source_end_ms.max(shot.source_start_ms + 1),
                ));
            }
        }
        for &index in &ordered {
            if !is_mutable(content.shots[index].order_index) {
                continue;
            }
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
                    crate::storyboard::length::source_span_ms(shot).max(1),
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
            crate::storyboard::length::set_source_range(shot, start, end);
            occupied.push((start, end));
        }
        // 窗内仍交叠：仅当参与均分的镜头都可改写才 pack，避免顺带改冻结镜头。
        if window_constrained_ranges_overlap(content, indices) {
            let packable = indices
                .iter()
                .all(|&index| is_mutable(content.shots[index].order_index));
            if packable {
                pack_overlapping_shots_within_windows(content, indices, pick_map);
            }
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
            crate::storyboard::length::set_source_range(shot, next_start, next_end);
        }
    }
}

/// 旁白句界托底：没有 TTS 逐拍时间戳时，按语言估算每镜口播，再收进成片目标。
#[cfg(test)]
fn apply_narration_phrase_duration_floor(
    content: &mut StoryboardContent,
    pick_map: &HashMap<i64, (crate::storyboard::multimodal::Phase4ContentWindow, bool)>,
) {
    apply_narration_phrase_duration_floor_scoped(content, pick_map, None);
}

pub(crate) fn apply_narration_phrase_duration_floor_scoped(
    content: &mut StoryboardContent,
    pick_map: &HashMap<i64, (crate::storyboard::multimodal::Phase4ContentWindow, bool)>,
    mutable: Option<&HashSet<i64>>,
) {
    let mut planned: Vec<(usize, i64)> = Vec::new();
    for (index, shot) in content.shots.iter().enumerate() {
        if let Some(allowed) = mutable {
            if !allowed.contains(&shot.order_index) {
                continue;
            }
        }
        let narration = shot.narration_text.trim();
        if narration.is_empty() || !pick_map.contains_key(&shot.order_index) {
            continue;
        }
        planned.push((index, estimated_narration_ms(narration)));
    }
    if planned.is_empty() {
        return;
    }
    let weight_sum = planned.iter().map(|(_, weight)| *weight).sum::<i64>().max(1);
    let frozen_ms = content
        .shots
        .iter()
        .enumerate()
        .filter(|(index, _)| planned.iter().all(|(planned_index, _)| planned_index != index))
        .map(|(_, shot)| shot.duration_ms.max(0))
        .sum::<i64>();
    let budget = content
        .target_duration_ms
        .saturating_sub(frozen_ms)
        .max(planned.len() as i64);
    let mut remaining = budget;
    for (offset, (index, weight)) in planned.iter().enumerate() {
        let share = if offset + 1 == planned.len() {
            remaining
        } else {
            let share = ((*weight as i128 * budget as i128) / weight_sum as i128) as i64;
            share.min(remaining.saturating_sub((planned.len() - offset - 1) as i64))
        }
        .max(1);
        remaining = remaining.saturating_sub(share);
        let shot = &mut content.shots[*index];
        let Some((window, _)) = pick_map.get(&shot.order_index) else {
            continue;
        };
        let new_end = (shot.source_start_ms + share).min(window.end_ms);
        if new_end > shot.source_end_ms {
            crate::storyboard::length::set_source_range(shot, shot.source_start_ms, new_end);
        }
        shot.duration_ms = share;
    }
}

/// 按文字本身区分中文、英文和其他音节，不用统一的「两字 300 毫秒」。
/// 中文按字，英文按词，假名和谚文按音节；标点不计。
fn estimated_narration_ms(text: &str) -> i64 {
    const CJK_MS: f64 = 110.0;
    const LATIN_WORD_MS: f64 = 300.0;
    const KANA_MS: f64 = 80.0;
    const HANGUL_MS: f64 = 150.0;
    let mut cjk = 0.0_f64;
    let mut latin_words = 0.0_f64;
    let mut kana = 0.0_f64;
    let mut hangul = 0.0_f64;
    let mut latin_run = false;
    for ch in text.chars() {
        if ('\u{4e00}'..='\u{9fff}').contains(&ch) {
            latin_run = false;
            cjk += 1.0;
        } else if ('\u{3040}'..='\u{30ff}').contains(&ch) {
            latin_run = false;
            kana += 1.0;
        } else if ('\u{ac00}'..='\u{d7af}').contains(&ch) {
            latin_run = false;
            hangul += 1.0;
        } else if ch.is_ascii_alphanumeric() {
            if !latin_run {
                latin_words += 1.0;
                latin_run = true;
            }
        } else {
            latin_run = false;
        }
    }
    let ms = cjk * CJK_MS + latin_words * LATIN_WORD_MS + kana * KANA_MS + hangul * HANGUL_MS;
    if ms <= 0.0 {
        300
    } else {
        ms.round() as i64
    }
}

/// 镜头时间范围与池候选对照。
fn shot_range_ms(shot: &StoryboardShot) -> (i64, i64) {
    (shot.source_start_ms, shot.source_end_ms)
}

fn lookup_pool_source<'a>(
    rough: &'a RoughStoryboard,
    shot: &StoryboardShot,
) -> Option<&'a StoryboardSource> {
    let pool = rough
        .candidate_pools
        .iter()
        .find(|pool| pool.beat_id == shot.beat_id)?;
    pool.candidates
        .iter()
        .find(|candidate| {
            if candidate.asset_id != shot.asset_id {
                return false;
            }
            match (&candidate.segment, shot.segment_id.as_deref()) {
                (Some(segment), Some(segment_id)) => segment.id == segment_id,
                (None, None) => true,
                (Some(segment), None) => {
                    segment.start_ms == shot.source_start_ms && segment.end_ms == shot.source_end_ms
                }
                (None, Some(_)) => false,
            }
        })
        .or_else(|| {
            pool.candidates.iter().find(|candidate| {
                candidate.asset_id == shot.asset_id && candidate.segment.is_none()
            })
        })
}

fn shots_are_similar(
    rough: &RoughStoryboard,
    left: &StoryboardShot,
    right: &StoryboardShot,
) -> bool {
    if left.asset_id == right.asset_id {
        if left.segment_id.is_some() && left.segment_id == right.segment_id {
            return true;
        }
        if ranges_overlap(shot_range_ms(left), shot_range_ms(right)) {
            return true;
        }
    }
    match (
        lookup_pool_source(rough, left),
        lookup_pool_source(rough, right),
    ) {
        (Some(left_source), Some(right_source)) => sources_are_similar(left_source, right_source),
        _ => false,
    }
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
                "pick 1-3 assets from each covered beat's candidate pool",
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
                        "pick candidateIndexes only from the beat's candidate pool",
                        "keep the beat at 1-3 shots using distinct pool candidates",
                    ]),
                );
            }
            if let Some(segment_id) = shot.segment_id.as_deref() {
                let segment_in_pool = rough
                    .candidate_pools
                    .iter()
                    .find(|pool| pool.beat_id == beat_id)
                    .map(|pool| {
                        pool.candidates.iter().any(|candidate| {
                            candidate.asset_id == shot.asset_id
                                && candidate
                                    .segment
                                    .as_ref()
                                    .is_some_and(|segment| segment.id == segment_id)
                        })
                    })
                    .unwrap_or(false);
                if !segment_in_pool {
                    issues.push(
                        StoryboardIssue::new(
                            "outside_candidate_segment",
                            format!(
                                "Shot {} for beat '{beat_id}' uses segment '{segment_id}' on asset '{}', which is outside that beat's Phase 2 pool.",
                                offset + 1,
                                shot.asset_id
                            ),
                            true,
                        )
                        .for_shots(vec![shot.order_index])
                        .allowing(vec![
                            "pick candidateIndexes only from that beat's candidate pool cards",
                        ]),
                    );
                }
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
        if group_count > 3 {
            issues.push(
                StoryboardIssue::new(
                    "beat_above_max_shots",
                    format!(
                        "Beat '{beat_id}' has {group_count} shots; keep each beat to at most 3 shots."
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
                .allowing(vec!["drop extra shots until at most 3 remain"]),
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

    // 已用片段相似/交叠硬拒（含跨 beat、含非相邻）；同片不同非相似段允许相邻。
    for left_index in 0..final_content.shots.len() {
        for right_index in (left_index + 1)..final_content.shots.len() {
            let left = &final_content.shots[left_index];
            let right = &final_content.shots[right_index];
            if shots_are_similar(rough, left, right) {
                issues.push(
                    StoryboardIssue::new(
                        "similar_used_segment",
                        format!(
                            "Shots {} and {} use overlapping or visually similar footage; later shots must pick a dissimilar segment.",
                            left.order_index, right.order_index
                        ),
                        true,
                    )
                    .for_shots(vec![left.order_index, right.order_index])
                    .allowing(vec![
                        "replace one shot with a dissimilar candidateIndex from its beat pool",
                        "or mark the weaker beat uncovered=true when no dissimilar alternate remains",
                    ]),
                );
            }
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
                            "replace {excess} of these shots using candidateIndexes for different assets from their beat pools so '{asset_id}' is used at most {max_allowed} times"
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
