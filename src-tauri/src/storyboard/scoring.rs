//! 候选片段综合评分与排序。
//!
//! 为每个候选片段计算综合分数（画面质量、时长匹配、语义相关性、多样性、新鲜度），
//! 并按分数降序排序，供 storyboard 生成时优先选择高质量镜头。

use crate::models::{StoryboardBeat, StoryboardSource};
use crate::storyboard::semantic::ocr_is_meaningful;
use serde::{Deserialize, Serialize};

/// 候选片段评分结果，用于排序。
#[derive(Debug, Clone)]
pub(crate) struct ScoredCandidate {
    pub source: StoryboardSource,
    pub score: CandidateScore,
}

/// 分数分解：便于日志与 Phase 3 卡片展示；可随候选池持久化。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CandidateScore {
    pub total: f64,
    pub semantic: f64,
    pub lexical: f64,
    /// beat 文案 ↔ 片段画面（CLIP）相似度；缺失时为 0，不影响既有 bge/词面路径。
    #[serde(default)]
    pub clip: f64,
    pub quality: f64,
    pub duration: f64,
    pub freshness: f64,
    /// 景别与 beat 要求的轻量匹配加分；只有片段候选带景别时才非 0。
    #[serde(default)]
    pub shot_type: f64,
    pub has_evidence: bool,
    #[serde(default)]
    pub matched_keywords: Vec<String>,
}

impl CandidateScore {
    pub(crate) fn retrieval_score_pct(&self) -> i64 {
        (self.total.clamp(0.0, 100.0)).round() as i64
    }
}

/// 为所有候选片段打分并排序（降序）。
///
/// 评分维度：
/// - 语义相关性（0-50分）：余弦与词面各最多 25；缺一侧时另一侧放大到 50
/// - CLIP 图文（0-25分）：beat 文案与片段代表帧；缺模型/向量时为 0
/// - 画面质量（0-10分）：来自 visual_quality_score
/// - 时长匹配度（0-10分）：候选时长与目标时长的适配度
/// - 当前 Storyboard 复用惩罚（每次 -15 分）：已经选过的素材累计降权
/// - 连续复用惩罚（额外 -30 分）：避免相邻镜头继续使用同一视频
/// - 新鲜度（0-5分）：根据项目内使用次数降权
///
/// 无视觉证据且无有效 OCR 的素材排在有证据候选之后。
pub(crate) fn rank_segment_candidates(
    candidates: Vec<StoryboardSource>,
    beat: &StoryboardBeat,
    target_duration_ms: i64,
    prior_selections: &[String], // 已选镜头的 asset_id 列表
    usage_counts: &std::collections::HashMap<String, i32>, // 素材在项目其他 timeline 的去重使用次数
    beat_embedding: Option<&[f32]>,
    beat_clip_embedding: Option<&[f32]>,
) -> Vec<ScoredCandidate> {
    let mut scored: Vec<_> = candidates
        .into_iter()
        .map(|candidate| {
            let score = calculate_candidate_score(
                &candidate,
                beat,
                target_duration_ms,
                prior_selections,
                usage_counts,
                beat_embedding,
                beat_clip_embedding,
            );
            ScoredCandidate {
                source: candidate,
                score,
            }
        })
        .collect();

    scored.sort_by(
        |a, b| match b.score.has_evidence.cmp(&a.score.has_evidence) {
            std::cmp::Ordering::Equal => b
                .score
                .total
                .partial_cmp(&a.score.total)
                .unwrap_or(std::cmp::Ordering::Equal),
            other => other,
        },
    );
    scored
}

fn calculate_candidate_score(
    candidate: &StoryboardSource,
    beat: &StoryboardBeat,
    target_duration_ms: i64,
    prior_selections: &[String],
    usage_counts: &std::collections::HashMap<String, i32>,
    beat_embedding: Option<&[f32]>,
    beat_clip_embedding: Option<&[f32]>,
) -> CandidateScore {
    let has_evidence = candidate_has_evidence(candidate);
    let (semantic, lexical, matched_keywords) =
        semantic_match_parts(candidate, beat, beat_embedding);
    let clip = clip_match_score(candidate, beat_clip_embedding);

    let quality = candidate.visual_quality_score.unwrap_or(0.5) * 10.0;

    let duration = duration_score(candidate, target_duration_ms);
    let shot_type = shot_type_score(candidate, beat);

    let usage_count = usage_counts.get(&candidate.asset_id).copied().unwrap_or(0);
    let freshness = 5.0 / (1.0 + usage_count.max(0) as f64);

    let mut total = semantic + lexical + clip + quality + duration + freshness + shot_type;

    let current_storyboard_uses = prior_selections
        .iter()
        .filter(|asset_id| *asset_id == &candidate.asset_id)
        .count();
    total -= current_storyboard_uses as f64 * 15.0;

    if let Some(last_asset) = prior_selections.last() {
        if last_asset == &candidate.asset_id {
            total -= 30.0;
        }
    }

    CandidateScore {
        total,
        semantic,
        lexical,
        clip,
        quality,
        duration,
        freshness,
        shot_type,
        has_evidence,
        matched_keywords,
    }
}

fn clip_match_score(candidate: &StoryboardSource, beat_clip_embedding: Option<&[f32]>) -> f64 {
    beat_clip_embedding
        .zip(candidate.segment_clip_embedding.as_deref())
        .and_then(|(query, image)| crate::storyboard::semantic::cosine_similarity(query, image))
        .map(|similarity| similarity.max(0.0) * 25.0)
        .unwrap_or(0.0)
}

/// 时长匹配：片段候选按片段跨度衡量，整素材候选沿用最长场景段/整片时长。
/// 跨度不足目标的 60% 时额外扣分，避免选出撑不满 beat 的碎片。
fn duration_score(candidate: &StoryboardSource, target_duration_ms: i64) -> f64 {
    if candidate.kind != "video" {
        return 10.0;
    }
    let target = target_duration_ms.max(1) as f64;
    let available = match candidate.segment.as_ref() {
        Some(segment) => segment.span_ms() as f64,
        None => {
            let Some(duration) = candidate.duration_ms else {
                return 0.0;
            };
            candidate
                .scene_segments
                .iter()
                .map(|segment| segment.end_ms - segment.start_ms)
                .max()
                .unwrap_or(duration)
                .max(0) as f64
        }
    };
    let ratio = available / target;
    let score = ratio.min(1.0) * 10.0;
    if ratio < 0.6 {
        (score - 3.0).max(0.0)
    } else {
        score
    }
}

/// 景别轻量匹配：beat 要求特写/细节时偏向 close-up/detail，要求全景时偏向 wide。
fn shot_type_score(candidate: &StoryboardSource, beat: &StoryboardBeat) -> f64 {
    let Some(shot_type) = candidate
        .segment
        .as_ref()
        .and_then(|segment| segment.shot_type.as_deref())
    else {
        return 0.0;
    };
    let shot_type = shot_type.trim().to_ascii_lowercase();
    if shot_type.is_empty() {
        return 0.0;
    }
    let request = format!("{} {}", beat.required_visual, beat.purpose).to_ascii_lowercase();
    let wants_detail = request.contains("detail") || request.contains("特写");
    let wants_wide =
        request.contains("establishing") || request.contains("全景") || request.contains("wide");
    if wants_detail && (shot_type.contains("close") || shot_type.contains("detail")) {
        return 3.0;
    }
    if wants_wide && shot_type.contains("wide") {
        return 3.0;
    }
    0.0
}

fn candidate_has_evidence(candidate: &StoryboardSource) -> bool {
    let has_visual = candidate.visual_evidence.iter().any(|evidence| {
        !evidence.subjects.is_empty()
            || !evidence.actions.is_empty()
            || !evidence.products.is_empty()
            || evidence
                .scene
                .as_ref()
                .is_some_and(|scene| !scene.trim().is_empty())
    });
    let has_ocr = candidate
        .ocr_evidence
        .iter()
        .any(|item| ocr_is_meaningful(&item.text));
    has_visual || has_ocr
}

fn semantic_match_parts(
    candidate: &StoryboardSource,
    beat: &StoryboardBeat,
    beat_embedding: Option<&[f32]>,
) -> (f64, f64, Vec<String>) {
    // 片段候选优先用片段向量；片段向量缺失时回退素材级向量。
    let candidate_embedding = candidate
        .segment_embedding
        .as_deref()
        .or(candidate.evidence_embedding.as_deref());
    let cosine_available =
        beat_embedding
            .zip(candidate_embedding)
            .and_then(|(query, candidate_embedding)| {
                crate::storyboard::semantic::cosine_similarity(query, candidate_embedding)
            });

    let blob = evidence_blob(candidate);
    let evidence_has_cjk = blob
        .chars()
        .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch));
    let query = lexical_query_terms(beat, evidence_has_cjk);
    let matched_keywords = matched_lexical_terms(&query, &blob);
    let lexical_available = !query.is_empty();
    let hit_ratio = if lexical_available {
        matched_keywords.len() as f64 / query.len() as f64
    } else {
        0.0
    };

    match (cosine_available, lexical_available) {
        (Some(similarity), true) => (
            similarity.max(0.0) * 25.0,
            hit_ratio * 25.0,
            matched_keywords,
        ),
        (Some(similarity), false) => (similarity.max(0.0) * 50.0, 0.0, matched_keywords),
        (None, true) => (0.0, hit_ratio * 50.0, matched_keywords),
        (None, false) => {
            let fallback = ((candidate.visual_evidence.len()
                + candidate
                    .ocr_evidence
                    .iter()
                    .filter(|item| ocr_is_meaningful(&item.text))
                    .count()) as f64)
                .min(10.0)
                * 1.2;
            (fallback, 0.0, matched_keywords)
        }
    }
}

fn lexical_query_terms(beat: &StoryboardBeat, evidence_has_cjk: bool) -> Vec<String> {
    let mut terms = Vec::new();
    for keyword in &beat.visual_keywords {
        terms.extend(ascii_terms(keyword));
    }
    // requiredVisual / purpose：ASCII 词元始终参与；中文双字仅在证据含 CJK 时参与。
    let narrative = format!("{} {}", beat.required_visual, beat.purpose);
    for term in query_terms(&narrative) {
        let is_cjk = term
            .chars()
            .any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch));
        if is_cjk {
            if evidence_has_cjk {
                terms.push(term);
            }
        } else {
            terms.push(term);
        }
    }
    terms.sort();
    terms.dedup();
    terms
}

fn matched_lexical_terms(query: &[String], blob: &str) -> Vec<String> {
    query
        .iter()
        .filter(|term| blob.contains(term.as_str()))
        .cloned()
        .collect()
}

fn evidence_blob(candidate: &StoryboardSource) -> String {
    let mut parts = Vec::new();
    for evidence in &candidate.visual_evidence {
        parts.extend(evidence.subjects.iter().cloned());
        parts.extend(evidence.actions.iter().cloned());
        parts.extend(evidence.products.iter().cloned());
        if let Some(scene) = &evidence.scene {
            parts.push(scene.clone());
        }
    }
    parts.extend(
        candidate
            .ocr_evidence
            .iter()
            .filter(|item| ocr_is_meaningful(&item.text))
            .map(|item| item.text.clone()),
    );
    parts.join(" ").to_lowercase()
}

fn ascii_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut ascii = String::new();
    let flush = |value: &mut String, terms: &mut Vec<String>| {
        if value.len() >= 2 {
            terms.push(std::mem::take(value));
        } else {
            value.clear();
        }
    };
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            ascii.push(character.to_ascii_lowercase());
        } else {
            flush(&mut ascii, &mut terms);
        }
    }
    flush(&mut ascii, &mut terms);
    terms
}

fn query_terms(text: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut ascii = String::new();
    let mut cjk = String::new();
    let flush_ascii = |value: &mut String, terms: &mut Vec<String>| {
        if value.len() >= 2 {
            terms.push(std::mem::take(value));
        } else {
            value.clear();
        }
    };
    let flush_cjk = |value: &mut String, terms: &mut Vec<String>| {
        if value.is_empty() {
            return;
        }
        let characters = value.chars().collect::<Vec<_>>();
        if characters.len() == 1 {
            terms.push(std::mem::take(value));
            return;
        }
        terms.extend(
            characters
                .windows(2)
                .map(|pair| pair.iter().collect::<String>()),
        );
        value.clear();
    };
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            flush_cjk(&mut cjk, &mut terms);
            ascii.push(character.to_ascii_lowercase());
        } else if ('\u{4e00}'..='\u{9fff}').contains(&character) {
            flush_ascii(&mut ascii, &mut terms);
            cjk.push(character);
        } else {
            flush_ascii(&mut ascii, &mut terms);
            flush_cjk(&mut cjk, &mut terms);
        }
    }
    flush_ascii(&mut ascii, &mut terms);
    flush_cjk(&mut cjk, &mut terms);
    terms
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_source(
        asset_id: &str,
        kind: &str,
        duration_ms: Option<i64>,
        quality: f64,
    ) -> StoryboardSource {
        StoryboardSource {
            asset_id: asset_id.to_owned(),
            kind: kind.to_owned(),
            duration_ms,
            scene_segments: vec![],
            ocr_evidence: vec![],
            visual_evidence: vec![],
            visual_quality_score: Some(quality),
            evidence_embedding: None,
            keyframe_grid_path: None,
            keyframes: Vec::new(),
            source_path: None,
            segment: None,
            segment_embedding: None,
            segment_clip_embedding: None,
        }
    }

    fn test_beat() -> StoryboardBeat {
        StoryboardBeat {
            id: "beat-1".to_owned(),
            purpose: "test".to_owned(),
            required_visual: "factory line".to_owned(),
            visual_keywords: vec![],
            narration: String::new(),
            on_screen_text: String::new(),
        }
    }

    #[test]
    fn higher_quality_scores_higher() {
        let high = make_source("high", "video", Some(10_000), 0.9);
        let low = make_source("low", "video", Some(10_000), 0.3);

        let usage = std::collections::HashMap::new();
        let high_score =
            calculate_candidate_score(&high, &test_beat(), 10_000, &[], &usage, None, None);
        let low_score =
            calculate_candidate_score(&low, &test_beat(), 10_000, &[], &usage, None, None);

        assert!(high_score.total > low_score.total, "高质量素材应得分更高");
    }

    #[test]
    fn long_source_with_usable_window_is_not_penalized() {
        let perfect = make_source("perfect", "video", Some(5_000), 0.5);
        let too_long = make_source("long", "video", Some(50_000), 0.5);

        let usage = std::collections::HashMap::new();
        let perfect_score =
            calculate_candidate_score(&perfect, &test_beat(), 5_000, &[], &usage, None, None);
        let long_score =
            calculate_candidate_score(&too_long, &test_beat(), 5_000, &[], &usage, None, None);

        assert_eq!(
            perfect_score.total, long_score.total,
            "长素材能够容纳目标镜头时不应降权"
        );
    }

    #[test]
    fn consecutive_same_asset_penalized() {
        let candidate = make_source("asset-1", "video", Some(10_000), 0.8);
        let prior = vec!["asset-1".to_owned()];

        let usage = std::collections::HashMap::new();
        let penalized =
            calculate_candidate_score(&candidate, &test_beat(), 10_000, &prior, &usage, None, None);
        let normal =
            calculate_candidate_score(&candidate, &test_beat(), 10_000, &[], &usage, None, None);

        assert!(penalized.total < normal.total, "连续使用同一素材应被降权");
        assert!(
            (normal.total - penalized.total - 45.0).abs() < 0.1,
            "连续复用应包含累计 -15 分和额外 -30 分"
        );
    }

    #[test]
    fn non_consecutive_reuse_is_penalized() {
        let candidate = make_source("asset-1", "video", Some(10_000), 0.8);
        let prior = vec!["asset-1".to_owned(), "asset-2".to_owned()];

        let usage = std::collections::HashMap::new();
        let penalized =
            calculate_candidate_score(&candidate, &test_beat(), 10_000, &prior, &usage, None, None);
        let normal =
            calculate_candidate_score(&candidate, &test_beat(), 10_000, &[], &usage, None, None);

        assert!(
            (normal.total - penalized.total - 15.0).abs() < 0.1,
            "非连续的第二次使用也应累计扣 15 分"
        );
    }

    #[test]
    fn required_visual_ranks_matching_scene_first() {
        let mut factory = make_source("factory", "video", Some(10_000), 0.5);
        factory.visual_evidence = vec![crate::models::VisualEvidence {
            time_ms: Some(0),
            subjects: vec!["workers".to_owned()],
            scene: Some("factory production line".to_owned()),
            actions: vec!["inspecting materials".to_owned()],
            products: vec![],
            quality_notes: vec![],
            shot_type: None,

            camera_motion: None,

            segment_id: None,
        }];
        let mut office = make_source("office", "video", Some(10_000), 0.9);
        office.visual_evidence = vec![crate::models::VisualEvidence {
            time_ms: Some(0),
            subjects: vec!["staff".to_owned()],
            scene: Some("office meeting".to_owned()),
            actions: vec!["talking".to_owned()],
            products: vec![],
            quality_notes: vec![],
            shot_type: None,

            camera_motion: None,

            segment_id: None,
        }];
        let beat = StoryboardBeat {
            id: "beat-factory".to_owned(),
            purpose: "show the factory visit".to_owned(),
            required_visual: "factory production line inspection".to_owned(),
            visual_keywords: vec![],
            narration: String::new(),
            on_screen_text: String::new(),
        };
        let ranked = rank_segment_candidates(
            vec![office, factory],
            &beat,
            10_000,
            &[],
            &std::collections::HashMap::new(),
            None,
            None,
        );
        assert_eq!(ranked[0].source.asset_id, "factory");
    }

    #[test]
    fn chinese_beat_with_english_keywords_ranks_matching_asset_first() {
        let mut forklift = make_source("forklift", "video", Some(10_000), 0.4);
        forklift.visual_evidence = vec![crate::models::VisualEvidence {
            time_ms: Some(0),
            subjects: vec!["forklift operator".to_owned()],
            scene: Some("outdoor loading area".to_owned()),
            actions: vec!["operating forklift".to_owned()],
            products: vec!["yellow forklift".to_owned()],
            quality_notes: vec![],
            shot_type: None,

            camera_motion: None,

            segment_id: None,
        }];
        let mut office = make_source("office", "video", Some(10_000), 0.95);
        office.visual_evidence = vec![crate::models::VisualEvidence {
            time_ms: Some(0),
            subjects: vec!["staff".to_owned()],
            scene: Some("office meeting".to_owned()),
            actions: vec!["talking".to_owned()],
            products: vec![],
            quality_notes: vec![],
            shot_type: None,

            camera_motion: None,

            segment_id: None,
        }];
        let beat = StoryboardBeat {
            id: "beat-logistics".to_owned(),
            purpose: "展示物流发货效率".to_owned(),
            required_visual: "叉车装卸货物".to_owned(),
            visual_keywords: vec![
                "forklift".to_owned(),
                "pallet".to_owned(),
                "loading dock".to_owned(),
                "cargo".to_owned(),
            ],
            narration: String::new(),
            on_screen_text: String::new(),
        };
        let ranked = rank_segment_candidates(
            vec![office, forklift],
            &beat,
            10_000,
            &[],
            &std::collections::HashMap::new(),
            None,
            None,
        );
        assert_eq!(ranked[0].source.asset_id, "forklift");
        assert!(!ranked[0].score.matched_keywords.is_empty());
    }

    #[test]
    fn assets_without_evidence_rank_after_evidenced_candidates() {
        let bare = make_source("bare", "video", Some(10_000), 0.9);
        let mut evidenced = make_source("evidenced", "video", Some(10_000), 0.2);
        evidenced.visual_evidence = vec![crate::models::VisualEvidence {
            time_ms: Some(0),
            subjects: vec!["battery modules".to_owned()],
            scene: Some("battery testing rack".to_owned()),
            actions: vec![],
            products: vec!["battery".to_owned()],
            quality_notes: vec![],
            shot_type: None,

            camera_motion: None,

            segment_id: None,
        }];
        let beat = StoryboardBeat {
            id: "beat-battery".to_owned(),
            purpose: "展示电池测试".to_owned(),
            required_visual: "电池模组检测".to_owned(),
            visual_keywords: vec!["battery".to_owned(), "modules".to_owned()],
            narration: String::new(),
            on_screen_text: String::new(),
        };
        let ranked = rank_segment_candidates(
            vec![bare, evidenced],
            &beat,
            10_000,
            &[],
            &std::collections::HashMap::new(),
            None,
            None,
        );
        assert_eq!(ranked[0].source.asset_id, "evidenced");
        assert!(ranked[0].score.has_evidence);
        assert!(!ranked[1].score.has_evidence);
    }

    #[test]
    fn rank_sorts_by_score_descending() {
        let candidates = vec![
            make_source("low", "video", Some(10_000), 0.3),
            make_source("high", "video", Some(10_000), 0.9),
            make_source("mid", "video", Some(10_000), 0.6),
        ];

        let beat = StoryboardBeat {
            id: "beat-1".to_owned(),
            purpose: "test".to_owned(),
            required_visual: "test".to_owned(),
            visual_keywords: vec![],
            narration: String::new(),
            on_screen_text: String::new(),
        };

        let ranked = rank_segment_candidates(
            candidates,
            &beat,
            10_000,
            &[],
            &std::collections::HashMap::new(),
            None,
            None,
        );

        assert_eq!(ranked[0].source.asset_id, "high");
        assert_eq!(ranked[1].source.asset_id, "mid");
        assert_eq!(ranked[2].source.asset_id, "low");
    }

    #[test]
    fn previously_used_asset_loses_freshness_points() {
        let fresh = make_source("fresh", "video", Some(10_000), 0.5);
        let used = make_source("used", "video", Some(10_000), 0.5);
        let usage = std::collections::HashMap::from([("used".to_owned(), 3)]);

        let ranked = rank_segment_candidates(
            vec![used, fresh],
            &test_beat(),
            10_000,
            &[],
            &usage,
            None,
            None,
        );

        assert_eq!(ranked[0].source.asset_id, "fresh");
    }

    #[test]
    fn internal_embeddings_are_not_serialized_for_the_provider() {
        let mut source = make_source("embedded", "video", Some(10_000), 0.5);
        source.evidence_embedding = Some(vec![0.1, 0.2, 0.3]);
        source.keyframe_grid_path = Some("C:\\private\\grid.jpg".to_owned());

        let serialized = serde_json::to_value(source).expect("storyboard source should serialize");

        assert!(serialized.get("evidenceEmbedding").is_none());
        assert!(serialized.get("evidence_embedding").is_none());
        assert!(serialized.get("keyframeGridPath").is_none());
    }

    #[test]
    fn clip_similarity_adds_image_weight_when_present() {
        let mut matching = make_source("match", "video", Some(10_000), 0.5);
        matching.segment_clip_embedding = Some(vec![1.0, 0.0, 0.0]);
        let mut other = make_source("other", "video", Some(10_000), 0.5);
        other.segment_clip_embedding = Some(vec![0.0, 1.0, 0.0]);
        let beat_clip = [1.0_f32, 0.0, 0.0];
        let usage = std::collections::HashMap::new();
        let match_score = calculate_candidate_score(
            &matching,
            &test_beat(),
            10_000,
            &[],
            &usage,
            None,
            Some(&beat_clip),
        );
        let other_score = calculate_candidate_score(
            &other,
            &test_beat(),
            10_000,
            &[],
            &usage,
            None,
            Some(&beat_clip),
        );
        assert!(match_score.clip > other_score.clip);
        assert!(match_score.total > other_score.total);
        assert!((match_score.clip - 25.0).abs() < 0.01);
    }
}
