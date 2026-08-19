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
/// - 语义相关性（0-50分）：当前粗略用 visual_evidence/ocr 命中数估算
/// - 画面质量（0-25分）：来自 visual_quality_score
/// - 时长匹配度（0-15分）：候选时长与目标时长的适配度
/// - 多样性惩罚（-10分）：连续使用同一素材降权
/// - 新鲜度（0-10分）：根据项目内使用次数降权
pub(crate) fn rank_segment_candidates(
    candidates: Vec<StoryboardSource>,
    _beat: &StoryboardBeat,
    target_duration_ms: i64,
    prior_selections: &[String], // 已选镜头的 asset_id 列表
    _usage_counts: &std::collections::HashMap<String, i32>, // 素材在项目其他 timeline 的使用次数（TODO：从 DB 读取）
) -> Vec<ScoredCandidate> {
    let mut scored: Vec<_> = candidates
        .into_iter()
        .map(|candidate| {
            let score = calculate_candidate_score(&candidate, target_duration_ms, prior_selections);
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
    target_duration_ms: i64,
    prior_selections: &[String],
) -> f64 {
    let mut score = 0.0;

    // 1. 语义相关性（0-50分）
    // 当前粗略估算：visual_evidence 和 ocr_evidence 条目数
    let evidence_count = (candidate.visual_evidence.len() + candidate.ocr_evidence.len()) as f64;
    score += evidence_count.min(10.0) * 5.0;

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

    #[test]
    fn higher_quality_scores_higher() {
        let high = make_source("high", "video", Some(10_000), 0.9);
        let low = make_source("low", "video", Some(10_000), 0.3);

        let high_score = calculate_candidate_score(&high, 10_000, &[]);
        let low_score = calculate_candidate_score(&low, 10_000, &[]);

        assert!(high_score > low_score, "高质量素材应得分更高");
    }

    #[test]
    fn better_duration_match_scores_higher() {
        let perfect = make_source("perfect", "video", Some(5_000), 0.5);
        let too_long = make_source("long", "video", Some(50_000), 0.5);

        let perfect_score = calculate_candidate_score(&perfect, 5_000, &[]);
        let long_score = calculate_candidate_score(&too_long, 5_000, &[]);

        assert!(perfect_score > long_score, "时长完美匹配应得分更高");
    }

    #[test]
    fn consecutive_same_asset_penalized() {
        let candidate = make_source("asset-1", "video", Some(10_000), 0.8);
        let prior = vec!["asset-1".to_owned()];

        let penalized = calculate_candidate_score(&candidate, 10_000, &prior);
        let normal = calculate_candidate_score(&candidate, 10_000, &[]);

        assert!(penalized < normal, "连续使用同一素材应被降权");
        assert!((normal - penalized - 10.0).abs() < 0.1, "降权应为 -10 分");
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
