//! Storyboard 单步重试预算：传输失败与语义/校验失败分开计数。
//!
//! 本 Phase 未过关不得进入下一 Phase；语义失败必须带上一版快照再修。

use crate::models::StoryboardShot;
use crate::storyboard::repair::{
    frozen_shot_indices, RepairPacket, RepairRecord, ShotSnapshot, StoryboardIssue,
};

pub(crate) const DEFAULT_SEMANTIC_ATTEMPTS: usize = 3;
pub(crate) const DEFAULT_TRANSPORT_ATTEMPTS: usize = 2;

/// 单步（Phase）重试预算。
#[derive(Debug, Clone)]
pub(crate) struct StepRetryBudget {
    pub phase_name: String,
    semantic_limit: usize,
    transport_limit: usize,
    semantic_used: usize,
    transport_used: usize,
    /// 语义预算用尽后，允许额外 1 次「带快照的校验尾修」。
    validation_tail_used: bool,
}

impl StepRetryBudget {
    pub(crate) fn new(phase_name: impl Into<String>) -> Self {
        Self {
            phase_name: phase_name.into(),
            semantic_limit: DEFAULT_SEMANTIC_ATTEMPTS,
            transport_limit: DEFAULT_TRANSPORT_ATTEMPTS,
            semantic_used: 0,
            transport_used: 0,
            validation_tail_used: false,
        }
    }

    pub(crate) fn semantic_attempt_number(&self) -> usize {
        self.semantic_used.saturating_add(1)
    }

    #[cfg(test)]
    pub(crate) fn semantic_used(&self) -> usize {
        self.semantic_used
    }

    pub(crate) fn can_retry_transport(&self) -> bool {
        self.transport_used < self.transport_limit
    }

    pub(crate) fn can_retry_semantic(&self) -> bool {
        self.semantic_used < self.semantic_limit
    }

    /// 语义轮已用尽、尚未用过尾修、且本轮有可快照候选时，允许 +1 校验尾修。
    pub(crate) fn can_validation_tail(&self, has_snapshot: bool) -> bool {
        has_snapshot && !self.validation_tail_used && self.semantic_used >= self.semantic_limit
    }

    pub(crate) fn record_transport_failure(&mut self) {
        self.transport_used = self.transport_used.saturating_add(1);
    }

    pub(crate) fn record_semantic_failure(&mut self) {
        self.semantic_used = self.semantic_used.saturating_add(1);
    }

    pub(crate) fn record_validation_tail(&mut self) {
        self.validation_tail_used = true;
    }

    pub(crate) fn exhausted_message(&self, last_issue: &str) -> String {
        format!(
            "{} failed after {} semantic and {} transport attempts: {last_issue}",
            self.phase_name, self.semantic_used, self.transport_used
        )
    }
}

/// 判断错误是否像传输/解析失败（不计入语义预算）。
pub(crate) fn is_transport_or_parse_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("连接")
        || lower.contains("os error")
        || lower.contains("读取响应失败")
        || lower.contains("did not contain json")
        || lower.contains("json did not match")
        || lower.contains("json was invalid")
        || lower.contains("schema")
        || lower.contains("request_failed")
        || lower.contains("connection")
        || lower.contains("reset")
}

pub(crate) fn shot_snapshots(shots: &[StoryboardShot]) -> Vec<ShotSnapshot> {
    shots
        .iter()
        .map(|shot| ShotSnapshot {
            shot_index: shot.order_index,
            beat_id: shot.beat_id.clone(),
            asset_id: shot.asset_id.clone(),
            duration_ms: shot.duration_ms,
            source_start_ms: shot.source_start_ms,
            source_end_ms: shot.source_end_ms,
        })
        .collect()
}

pub(crate) fn build_repair_packet(
    attempt: usize,
    issues: Vec<StoryboardIssue>,
    candidate_shots: &[StoryboardShot],
    previous: Option<&RepairPacket>,
) -> RepairPacket {
    let frozen = frozen_shot_indices(candidate_shots.iter().map(|shot| shot.order_index), &issues);
    let previous_shots = shot_snapshots(candidate_shots);
    let unresolved_kinds = issues
        .iter()
        .map(|issue| issue.kind.clone())
        .collect::<std::collections::HashSet<_>>();
    let repair_history = if let Some(previous) = previous {
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
            RepairRecord::new(
                attempt,
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
                RepairRecord::new(
                    attempt,
                    issue.kind.clone(),
                    issue.affected_shots.clone(),
                    false,
                )
            })
            .collect()
    };
    RepairPacket::with_context(attempt, issues, previous_shots, frozen, repair_history)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transport_failures_do_not_consume_semantic_budget() {
        let mut budget = StepRetryBudget::new("phase-test");
        assert!(budget.can_retry_transport());
        budget.record_transport_failure();
        budget.record_transport_failure();
        assert!(!budget.can_retry_transport());
        assert!(budget.can_retry_semantic());
        assert_eq!(budget.semantic_used, 0);
    }

    #[test]
    fn validation_tail_only_after_semantic_exhausted_with_snapshot() {
        let mut budget = StepRetryBudget::new("phase-test");
        for _ in 0..DEFAULT_SEMANTIC_ATTEMPTS {
            budget.record_semantic_failure();
        }
        assert!(!budget.can_retry_semantic());
        assert!(!budget.can_validation_tail(false));
        assert!(budget.can_validation_tail(true));
        budget.record_validation_tail();
        assert!(!budget.can_validation_tail(true));
    }

    #[test]
    fn transport_error_classifier_catches_timeout_and_schema() {
        assert!(is_transport_or_parse_error(
            "自定义 API 读取响应失败: timeout"
        ));
        assert!(is_transport_or_parse_error(
            "Phase 3 JSON did not match StoryboardContent schema."
        ));
        assert!(!is_transport_or_parse_error(
            "Beat 'x' has 1 shot(s); every covered beat must expand to at least 2"
        ));
    }
}
