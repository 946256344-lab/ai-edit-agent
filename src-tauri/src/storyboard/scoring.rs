//! 候选片段综合评分与排序。
//!
//! 为每个候选片段计算综合分数（画面质量、时长匹配、语义相关性、多样性、新鲜度），
//! 并按分数降序排序，供 storyboard 生成时优先选择高质量镜头。

use crate::models::{StoryboardBeat, StoryboardSource};

/// 候选片段评分结果，用于排序。
#[derive(Debug, Clone)]
pub(crate) struct ScoredCandidate {
    pub source: StoryboardSource,
    pub score: f64,
}

/// 为所有候选片段打分并排序（降序）。
///
/// 评分维度：
/// - 语义相关性（0-30分）：优先使用本地向量相似度，无向量时使用词面重合
/// - 画面质量（0-25分）：来自 visual_quality_score
/// - 时长匹配度（0-15分）：候选时长与目标时长的适配度
/// - 多样性惩罚（-10分）：连续使用同一素材降权
/// - 新鲜度（0-10分）：根据项目内使用次数降权
pub(crate) fn rank_segment_candidates(
    candidates: Vec<StoryboardSource>,
    beat: &StoryboardBeat,
    target_duration_ms: i64,
    prior_selections: &[String], // 已选镜头的 asset_id 列表
    usage_counts: &std::collections::HashMap<String, i32>, // 素材在项目其他 timeline 的去重使用次数
    beat_embedding: Option<&[f32]>,
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
            );
            ScoredCandidate {
                source: candidate,
                score,
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored
}

fn calculate_candidate_score(
    candidate: &StoryboardSource,
    beat: &StoryboardBeat,
    target_duration_ms: i64,
    prior_selections: &[String],
    usage_counts: &std::collections::HashMap<String, i32>,
    beat_embedding: Option<&[f32]>,
) -> f64 {
    let mut score = 0.0;

    // 1. 语义相关性（0-30分）：本地向量优先，缺失或不兼容时保留词面降级。
    score += semantic_match_score(candidate, beat, beat_embedding);

    // 2. 画面质量（0-25分）
    let quality = candidate.visual_quality_score.unwrap_or(0.5);
    score += quality * 25.0;

    // 3. 时长匹配度（0-15分）
    if candidate.kind == "video" {
        if let Some(duration) = candidate.duration_ms {
            let target = target_duration_ms.max(1) as f64;
            let actual = duration.max(1) as f64;
            let ratio = if actual >= target {
                target / actual
            } else {
                actual / target
            };
            score += ratio * 15.0;
        }
    } else {
        // 图片素材时长灵活，给满分
        score += 15.0;
    }

    // 4. 多样性惩罚（-10分）
    if let Some(last_asset) = prior_selections.last() {
        if last_asset == &candidate.asset_id {
            score -= 10.0; // 连续使用同一素材降权
        }
    }

    // 5. 新鲜度（0-10分）：每个剪辑任务只计一次，使用越多分数越低。
    let usage_count = usage_counts.get(&candidate.asset_id).copied().unwrap_or(0);
    score += 10.0 / (1.0 + usage_count.max(0) as f64);

    score
}

fn semantic_match_score(
    candidate: &StoryboardSource,
    beat: &StoryboardBeat,
    beat_embedding: Option<&[f32]>,
) -> f64 {
    if let (Some(query), Some(candidate_embedding)) =
        (beat_embedding, candidate.evidence_embedding.as_deref())
    {
        if let Some(similarity) =
            crate::storyboard::semantic::cosine_similarity(query, candidate_embedding)
        {
            return similarity.max(0.0) * 30.0;
        }
    }
    let query = query_terms(&format!("{} {}", beat.required_visual, beat.purpose));
    if query.is_empty() {
        return ((candidate.visual_evidence.len() + candidate.ocr_evidence.len()) as f64).min(10.0)
            * 1.2;
    }
    let blob = evidence_blob(candidate);
    let hits = query
        .iter()
        .filter(|term| blob.contains(term.as_str()))
        .count();
    (hits as f64 / query.len() as f64) * 30.0
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
    parts.extend(candidate.ocr_evidence.iter().map(|item| item.text.clone()));
    parts.join(" ").to_lowercase()
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
        }
    }

    fn test_beat() -> StoryboardBeat {
        StoryboardBeat {
            id: "beat-1".to_owned(),
            purpose: "test".to_owned(),
            required_visual: "factory line".to_owned(),
            narration: String::new(),
        }
    }

    #[test]
    fn higher_quality_scores_higher() {
        let high = make_source("high", "video", Some(10_000), 0.9);
        let low = make_source("low", "video", Some(10_000), 0.3);

        let usage = std::collections::HashMap::new();
        let high_score = calculate_candidate_score(&high, &test_beat(), 10_000, &[], &usage, None);
        let low_score = calculate_candidate_score(&low, &test_beat(), 10_000, &[], &usage, None);

        assert!(high_score > low_score, "高质量素材应得分更高");
    }

    #[test]
    fn better_duration_match_scores_higher() {
        let perfect = make_source("perfect", "video", Some(5_000), 0.5);
        let too_long = make_source("long", "video", Some(50_000), 0.5);

        let usage = std::collections::HashMap::new();
        let perfect_score =
            calculate_candidate_score(&perfect, &test_beat(), 5_000, &[], &usage, None);
        let long_score =
            calculate_candidate_score(&too_long, &test_beat(), 5_000, &[], &usage, None);

        assert!(perfect_score > long_score, "时长完美匹配应得分更高");
    }

    #[test]
    fn consecutive_same_asset_penalized() {
        let candidate = make_source("asset-1", "video", Some(10_000), 0.8);
        let prior = vec!["asset-1".to_owned()];

        let usage = std::collections::HashMap::new();
        let penalized =
            calculate_candidate_score(&candidate, &test_beat(), 10_000, &prior, &usage, None);
        let normal = calculate_candidate_score(&candidate, &test_beat(), 10_000, &[], &usage, None);

        assert!(penalized < normal, "连续使用同一素材应被降权");
        assert!((normal - penalized - 10.0).abs() < 0.1, "降权应为 -10 分");
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
        }];
        let mut office = make_source("office", "video", Some(10_000), 0.9);
        office.visual_evidence = vec![crate::models::VisualEvidence {
            time_ms: Some(0),
            subjects: vec!["staff".to_owned()],
            scene: Some("office meeting".to_owned()),
            actions: vec!["talking".to_owned()],
            products: vec![],
            quality_notes: vec![],
        }];
        let beat = StoryboardBeat {
            id: "beat-factory".to_owned(),
            purpose: "show the factory visit".to_owned(),
            required_visual: "factory production line inspection".to_owned(),
            narration: String::new(),
        };
        let ranked = rank_segment_candidates(
            vec![office, factory],
            &beat,
            10_000,
            &[],
            &std::collections::HashMap::new(),
            None,
        );
        assert_eq!(ranked[0].source.asset_id, "factory");
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
            narration: String::new(),
        };

        let ranked = rank_segment_candidates(
            candidates,
            &beat,
            10_000,
            &[],
            &std::collections::HashMap::new(),
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

        let ranked =
            rank_segment_candidates(vec![used, fresh], &test_beat(), 10_000, &[], &usage, None);

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
}
