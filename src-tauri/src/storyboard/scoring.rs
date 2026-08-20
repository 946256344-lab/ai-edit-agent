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
/// - 语义相关性（0-50分）：`requiredVisual`/`purpose` 与画面证据、OCR 的词面重合
/// - 画面质量（0-25分）：来自 visual_quality_score
/// - 时长匹配度（0-15分）：候选时长与目标时长的适配度
/// - 多样性惩罚（-10分）：连续使用同一素材降权
/// - 新鲜度（0-10分）：根据项目内使用次数降权
pub(crate) fn rank_segment_candidates(
    candidates: Vec<StoryboardSource>,
    beat: &StoryboardBeat,
    target_duration_ms: i64,
    prior_selections: &[String], // 已选镜头的 asset_id 列表
    _usage_counts: &std::collections::HashMap<String, i32>, // 素材在项目其他 timeline 的使用次数（TODO：从 DB 读取）
) -> Vec<ScoredCandidate> {
    let mut scored: Vec<_> = candidates
        .into_iter()
        .map(|candidate| {
            let score =
                calculate_candidate_score(&candidate, beat, target_duration_ms, prior_selections);
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
) -> f64 {
    let mut score = 0.0;

    // 1. 语义相关性（0-50分）：按当前 beat 的画面要求匹配，而不是全局证据条数
    score += semantic_match_score(candidate, beat);

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

    // 5. 新鲜度（0-10分）
    // TODO: 从 usage_counts 读取该素材在项目其他 timeline 的使用次数
    // 当前暂不实现，预留接口
    score += 10.0; // 默认给满分

    score
}

fn semantic_match_score(candidate: &StoryboardSource, beat: &StoryboardBeat) -> f64 {
    let query = query_terms(&format!("{} {}", beat.required_visual, beat.purpose));
    if query.is_empty() {
        return ((candidate.visual_evidence.len() + candidate.ocr_evidence.len()) as f64).min(10.0)
            * 2.0;
    }
    let blob = evidence_blob(candidate);
    let hits = query
        .iter()
        .filter(|term| blob.contains(term.as_str()))
        .count();
    (hits as f64 / query.len() as f64) * 50.0
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

        let high_score = calculate_candidate_score(&high, &test_beat(), 10_000, &[]);
        let low_score = calculate_candidate_score(&low, &test_beat(), 10_000, &[]);

        assert!(high_score > low_score, "高质量素材应得分更高");
    }

    #[test]
    fn better_duration_match_scores_higher() {
        let perfect = make_source("perfect", "video", Some(5_000), 0.5);
        let too_long = make_source("long", "video", Some(50_000), 0.5);

        let perfect_score = calculate_candidate_score(&perfect, &test_beat(), 5_000, &[]);
        let long_score = calculate_candidate_score(&too_long, &test_beat(), 5_000, &[]);

        assert!(perfect_score > long_score, "时长完美匹配应得分更高");
    }

    #[test]
    fn consecutive_same_asset_penalized() {
        let candidate = make_source("asset-1", "video", Some(10_000), 0.8);
        let prior = vec!["asset-1".to_owned()];

        let penalized = calculate_candidate_score(&candidate, &test_beat(), 10_000, &prior);
        let normal = calculate_candidate_score(&candidate, &test_beat(), 10_000, &[]);

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
        );

        assert_eq!(ranked[0].source.asset_id, "high");
        assert_eq!(ranked[1].source.asset_id, "mid");
        assert_eq!(ranked[2].source.asset_id, "low");
    }
}
