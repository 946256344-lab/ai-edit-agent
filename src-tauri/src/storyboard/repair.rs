//! Phase 3 修复包：把本地校验发现的问题结构化地回传给模型继续决策。
//!
//! 原则：语义决策（用哪个素材、拆几段、换什么字幕、怎么调节奏）交给模型；
//! Rust 只收集问题、维护修复记忆与局部冻结，并最后做机械兜底（时长/范围等无歧义修正）。

use serde::Serialize;
use std::collections::HashSet;

/// 一个校验问题。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoryboardIssue {
    /// 稳定的机器可读问题类型，模型可按类型做修复决策。
    pub(crate) kind: String,
    /// 人类可读的问题描述。
    pub(crate) message: String,
    /// 受影响的 shot 序号（order_index）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) affected_shots: Vec<i64>,
    /// 是否需要模型决策；false 表示 Rust 可直接机械修正，不回传模型。
    pub(crate) needs_model_decision: bool,
    /// 允许的修复方向（约束边界，不是操作手册；模型仍自主决定具体做法）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) allowed_changes: Vec<String>,
}

impl StoryboardIssue {
    pub(crate) fn new(
        kind: impl Into<String>,
        message: impl Into<String>,
        needs_model_decision: bool,
    ) -> Self {
        StoryboardIssue {
            kind: kind.into(),
            message: message.into(),
            affected_shots: Vec::new(),
            needs_model_decision,
            allowed_changes: Vec::new(),
        }
    }

    pub(crate) fn for_shots(mut self, shots: Vec<i64>) -> Self {
        self.affected_shots = shots;
        self
    }

    pub(crate) fn allowing<I: Into<String>>(mut self, changes: Vec<I>) -> Self {
        self.allowed_changes = changes.into_iter().map(Into::into).collect();
        self
    }
}

/// 修复记忆中的一条记录：模型在某一轮对哪些镜头做过什么尝试、结果如何。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepairRecord {
    /// 该尝试发生的轮次（1-based）。
    pub(crate) round: usize,
    /// 问题类型（与 StoryboardIssue.kind 同源）。
    pub(crate) kind: String,
    /// 涉及的 shot 序号。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) shots: Vec<i64>,
    /// 上一轮之后该问题是否已经消失（模型修复成功）。
    pub(crate) resolved: bool,
}

impl RepairRecord {
    pub(crate) fn new(
        round: usize,
        kind: impl Into<String>,
        shots: Vec<i64>,
        resolved: bool,
    ) -> Self {
        RepairRecord {
            round,
            kind: kind.into(),
            shots,
            resolved,
        }
    }
}

/// 上一轮候选的精简快照：让模型在独立请求中也能“看到自己上一版输出”。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShotSnapshot {
    pub(crate) shot_index: i64,
    pub(crate) beat_id: String,
    pub(crate) asset_id: String,
    pub(crate) duration_ms: i64,
    pub(crate) source_start_ms: i64,
    pub(crate) source_end_ms: i64,
}

/// 一轮修复包：携带上一轮候选的全部问题、已确认正确的镜头和修复记忆。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RepairPacket {
    /// 当前是第几次尝试（1-based，第一轮无修复包）。
    pub(crate) attempt: usize,
    pub(crate) issues: Vec<StoryboardIssue>,
    /// 上一轮候选的精简快照（修复轮中模型据此接着改，不重写全局）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) previous_shots: Vec<ShotSnapshot>,
    /// 已通过本地校验的 shot：模型应保持不变，除非修复其他列出的问题确实需要改动。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) frozen_shots: Vec<i64>,
    /// 修复记忆：之前各轮尝试过的问题类型、镜头与结果。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) repair_history: Vec<RepairRecord>,
}

impl RepairPacket {
    pub(crate) fn new(attempt: usize, issues: Vec<StoryboardIssue>) -> Self {
        RepairPacket {
            attempt,
            issues,
            previous_shots: Vec::new(),
            frozen_shots: Vec::new(),
            repair_history: Vec::new(),
        }
    }

    pub(crate) fn with_context(
        attempt: usize,
        issues: Vec<StoryboardIssue>,
        previous_shots: Vec<ShotSnapshot>,
        frozen_shots: Vec<i64>,
        repair_history: Vec<RepairRecord>,
    ) -> Self {
        RepairPacket {
            attempt,
            issues,
            previous_shots,
            frozen_shots,
            repair_history,
        }
    }

    /// 是否还有需要模型决策的问题（否则 Rust 可以机械兜底完成）。
    pub(crate) fn needs_model_decision(&self) -> bool {
        self.issues.iter().any(|issue| issue.needs_model_decision)
    }
}

/// 根据候选镜头与问题集合，计算已确认正确的（冻结）镜头。
/// 被任何 issue 点名的镜头视为待修复，未点名且存在的镜头视为已确认。
pub(crate) fn frozen_shot_indices(
    shot_indices: impl IntoIterator<Item = i64>,
    issues: &[StoryboardIssue],
) -> Vec<i64> {
    let flagged = issues
        .iter()
        .flat_map(|issue| issue.affected_shots.iter().copied())
        .collect::<HashSet<_>>();
    shot_indices
        .into_iter()
        .filter(|index| !flagged.contains(index))
        .collect()
}

/// 把修复包渲染成注入 prompt 的结构化修复指示。
///
/// 设计成「给编辑看的交接单」而非「操作手册」：列清楚哪些已确认不能乱动、
/// 之前试过什么、还有哪些边界问题，具体怎么修由模型自己判断。
pub(crate) fn repair_packet_prompt_block(packet: &RepairPacket) -> String {
    const MAX_REPAIR_ATTEMPTS: usize = 3;
    let mut lines = Vec::new();
    if packet.repair_history.is_empty() {
        lines.push(format!(
            "Previous attempt was REJECTED (attempt {}/{MAX_REPAIR_ATTEMPTS}).",
            packet.attempt
        ));
    } else {
        let resolved = packet
            .repair_history
            .iter()
            .filter(|record| record.resolved)
            .count();
        lines.push(format!(
            "You are in repair round {} of {MAX_REPAIR_ATTEMPTS}. Previous rounds resolved {resolved} issue(s); the issues below remain open.",
            packet.attempt
        ));
    }
    if !packet.frozen_shots.is_empty() {
        lines.push(format!(
            "FROZEN SHOTS (already validated — keep them exactly as-is unless fixing a listed issue truly requires changing them): {:?}",
            packet.frozen_shots
        ));
    }
    if !packet.previous_shots.is_empty() {
        let preview = packet
            .previous_shots
            .iter()
            .map(|shot| {
                format!(
                    "#{} beat={} asset={} dur={}ms src=[{}-{}]",
                    shot.shot_index,
                    shot.beat_id,
                    shot.asset_id,
                    shot.duration_ms,
                    shot.source_start_ms,
                    shot.source_end_ms
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        lines.push(format!(
            "PREVIOUS CANDIDATE SNAPSHOT (what you are repairing from): {preview}"
        ));
    }
    if !packet.repair_history.is_empty() {
        let history = packet
            .repair_history
            .iter()
            .map(|record| {
                format!(
                    "round {}: {} shots {:?} -> {}",
                    record.round,
                    record.kind,
                    record.shots,
                    if record.resolved {
                        "resolved"
                    } else {
                        "still open"
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        lines.push(format!("REPAIR MEMORY (what you already tried): {history}"));
    }
    let json = serde_json::to_string_pretty(packet).unwrap_or_default();
    lines.push(format!(
        "OPEN ISSUES to fix — you are the editor: resolve each one, but only touch shots that must change; do not rewrite the whole storyboard. Rules are boundaries, not micro-instructions, so choose your own concrete fix.\n```json\n{json}\n```"
    ));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_renders_as_readable_json_block() {
        let packet = RepairPacket::new(
            2,
            vec![StoryboardIssue::new(
                "duplicate_asset",
                "beat 'hook' reuses asset sox in shots 2 and 3.",
                true,
            )
            .for_shots(vec![2, 3])
            .allowing(vec![
                "replace shot 3 with a distinct candidate from the beat pool",
                "or merge shots 2 and 3 into one shot",
            ])],
        );
        let block = repair_packet_prompt_block(&packet);
        assert!(block.contains("Previous attempt was REJECTED (attempt 2"));
        assert!(block.contains("duplicate_asset"));
        assert!(block.contains("\"affectedShots\""));
        assert!(block.contains("2"));
        assert!(block.contains("3"));
        assert!(block.contains("replace shot 3"));
    }

    #[test]
    fn packet_needs_model_decision_only_when_semantic() {
        let mechanical = RepairPacket::new(
            1,
            vec![StoryboardIssue::new(
                "range_clamped",
                "shot range exceeded the source duration; clamped by Rust.",
                false,
            )],
        );
        assert!(!mechanical.needs_model_decision());
    }

    #[test]
    fn frozen_shots_exclude_every_flagged_shot() {
        let issues = vec![
            StoryboardIssue::new("duplicate_asset", "duplicate", true).for_shots(vec![2, 3]),
            StoryboardIssue::new("first_shot_replaced", "first changed", true).for_shots(vec![1]),
        ];
        let frozen = frozen_shot_indices(vec![1, 2, 3, 4, 5], &issues);
        assert_eq!(frozen, vec![4, 5]);
    }

    #[test]
    fn packet_with_context_renders_memory_and_snapshot_sections() {
        let packet = RepairPacket::with_context(
            3,
            vec![StoryboardIssue::new(
                "duplicate_asset_in_beat",
                "beat 'steady-schedule' reuses asset 'a1' in shot 4.",
                true,
            )
            .for_shots(vec![4])
            .allowing(vec![
                "replace the duplicated shot with another candidate from the same beat pool",
                "or merge the duplicate back into the previous shot",
            ])],
            vec![ShotSnapshot {
                shot_index: 1,
                beat_id: "beat-1".to_owned(),
                asset_id: "asset-a".to_owned(),
                duration_ms: 2_000,
                source_start_ms: 0,
                source_end_ms: 2_000,
            }],
            vec![1, 2, 3, 5],
            vec![
                RepairRecord::new(1, "outside_candidate_pool", vec![2], true),
                RepairRecord::new(2, "first_shot_replaced", vec![1], true),
            ],
        );
        let block = repair_packet_prompt_block(&packet);
        assert!(block.contains("repair round 3 of 3"));
        assert!(block.contains("FROZEN SHOTS"));
        assert!(block.contains("[1, 2, 3, 5]"));
        assert!(block.contains("PREVIOUS CANDIDATE SNAPSHOT"));
        assert!(block.contains("beat=beat-1"));
        assert!(block.contains("REPAIR MEMORY"));
        assert!(block.contains("round 1: outside_candidate_pool"));
        assert!(block.contains("round 2: first_shot_replaced"));
        assert!(block.contains("resolved"));
        assert!(block.contains("you are the editor"));
    }
}
