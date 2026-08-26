//! Native 循环的两套续步：中止修复与产物精炼。
//!
//! 修复只在可重试写工具失败后拦截提前自然语言结束，逼模型换参数或补前置条件。
//! 精炼只在产物已成功落地但仍带 qualityWarnings 时要求继续打磨。两者都不锁定
//! 固定目标、不把授权工具清单当成完成清单，也不自动重放副作用。

use serde_json::Value;

use super::schema::MAX_STEPS;

/// 同一轮里，可重试写失败后最多拦截几次“提前收工”的自然语言。
pub(super) const MAX_RECOVERY_CONTINUATIONS: usize = 2;

/// 同一轮里，产物已齐但质量缺口未闭合时最多拦截几次“提前收工”。
pub(super) const MAX_REFINE_CONTINUATIONS: usize = 2;

/// 本轮不可再修、应诚实结束或等人确认的失败码；即使工具标了 retryable 也不续步。
const NOT_IN_TURN_RECOVERABLE: &[&str] = &[
    "storyboard_confirmation_required",
    "user_restricted_tool",
    "tool_not_allowed",
    "unsafe_tool_result",
    "voice_provider_unconfigured",
    "voice_provider_unauthorized",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ContinuationKind {
    Recovery,
    Refinement,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecoveryFact {
    tool: String,
    code: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RefineFact {
    tool: String,
    code: String,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct ContinuationState {
    recovery_count: usize,
    refine_count: usize,
    last_recoverable: Vec<RecoveryFact>,
    last_incomplete: Vec<RefineFact>,
    consecutive_same_recovery: usize,
}

impl ContinuationState {
    /// 记录本模型步实际执行过的工具结果。无工具的自然语言步不要调用，以保留待修缺口。
    pub(super) fn record_step(&mut self, results: &[(String, Value, bool)]) {
        let previous_recovery = self.last_recoverable.first().cloned();
        let mut new_recovery = None;
        for (tool, result, is_observation) in results {
            if let Some(fact) = recovery_fact(tool, result, *is_observation) {
                self.last_recoverable
                    .retain(|existing| existing.tool != *tool);
                new_recovery.get_or_insert_with(|| fact.clone());
                self.last_recoverable.push(fact);
            } else if !*is_observation && result["status"].as_str().is_some() {
                self.last_recoverable
                    .retain(|existing| existing.tool != *tool);
            }

            if result["status"].as_str() == Some("ok") {
                let incomplete = refine_facts(tool, result);
                self.last_incomplete
                    .retain(|existing| existing.tool != *tool);
                self.last_incomplete.extend(incomplete);
            }
        }
        self.consecutive_same_recovery = match (previous_recovery.as_ref(), new_recovery.as_ref()) {
            (Some(previous), Some(current)) if previous == current => {
                self.consecutive_same_recovery.saturating_add(1).max(1)
            }
            (_, Some(_)) => 1,
            _ if self.last_recoverable.is_empty() => 0,
            _ => self.consecutive_same_recovery,
        };
        if self.last_recoverable.is_empty() {
            self.recovery_count = 0;
        }
        if self.last_incomplete.is_empty() {
            self.refine_count = 0;
        }
    }

    pub(super) fn decide(
        &self,
        step_number: usize,
        needs_confirmation: bool,
    ) -> Option<ContinuationKind> {
        if needs_confirmation || step_number >= MAX_STEPS {
            return None;
        }
        if !self.last_recoverable.is_empty() && self.recovery_count < MAX_RECOVERY_CONTINUATIONS {
            return Some(ContinuationKind::Recovery);
        }
        if !self.last_incomplete.is_empty() && self.refine_count < MAX_REFINE_CONTINUATIONS {
            return Some(ContinuationKind::Refinement);
        }
        None
    }

    pub(super) fn take_message(&mut self, kind: ContinuationKind) -> String {
        match kind {
            ContinuationKind::Recovery => {
                self.recovery_count = self.recovery_count.saturating_add(1);
                recovery_message(&self.last_recoverable, self.consecutive_same_recovery)
            }
            ContinuationKind::Refinement => {
                self.refine_count = self.refine_count.saturating_add(1);
                refine_message(&self.last_incomplete)
            }
        }
    }
}

fn recovery_fact(tool: &str, result: &Value, is_observation: bool) -> Option<RecoveryFact> {
    if is_observation || result["status"].as_str() != Some("failed") {
        return None;
    }
    if result["retryable"].as_bool() != Some(true) {
        return None;
    }
    let code = result["code"].as_str().unwrap_or("skill_execution_failed");
    if NOT_IN_TURN_RECOVERABLE.contains(&code) {
        return None;
    }
    Some(RecoveryFact {
        tool: tool.to_owned(),
        code: code.to_owned(),
    })
}

fn refine_facts(tool: &str, result: &Value) -> Vec<RefineFact> {
    if result["status"].as_str() != Some("ok") {
        return Vec::new();
    }
    let mut facts = Vec::new();
    collect_warning_codes(result.get("qualityWarnings"), tool, &mut facts);
    collect_warning_codes(
        result.pointer("/artifact/qualityWarnings"),
        tool,
        &mut facts,
    );
    facts
}

fn collect_warning_codes(value: Option<&Value>, tool: &str, facts: &mut Vec<RefineFact>) {
    let Some(Value::Array(items)) = value else {
        return;
    };
    for item in items {
        let Some(code) = warning_code(item) else {
            continue;
        };
        if facts
            .iter()
            .any(|fact| fact.tool == tool && fact.code == code)
        {
            continue;
        }
        facts.push(RefineFact {
            tool: tool.to_owned(),
            code,
        });
    }
}

fn warning_code(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        let code = text
            .rsplit(':')
            .next()
            .unwrap_or(text)
            .trim()
            .chars()
            .take(48)
            .collect::<String>();
        return (!code.is_empty()).then_some(code);
    }
    if value["severity"].as_str() == Some("info") {
        return None;
    }
    value["category"]
        .as_str()
        .map(|category| category.chars().take(48).collect::<String>())
        .filter(|category| !category.is_empty())
}

fn recovery_message(facts: &[RecoveryFact], consecutive_same: usize) -> String {
    let summary = facts
        .iter()
        .map(|fact| format!("{}:{}", fact.tool, fact.code))
        .take(6)
        .collect::<Vec<_>>()
        .join(", ");
    let mut message = format!(
        "The last write function failed with a retryable code ({summary}) and did not create the requested artifact. Do not claim success. Call an allowed function to recover: retry with adjusted arguments, satisfy a documented prerequisite, or use a different allowed function."
    );
    if consecutive_same >= 2 {
        message.push_str(
            " The same function and code already failed; do not repeat identical arguments.",
        );
    }
    message.push_str(" If you cannot recover, a later natural-language explanation will be accepted as an honest stop.");
    message
}

fn refine_message(facts: &[RefineFact]) -> String {
    let summary = facts
        .iter()
        .map(|fact| format!("{}:{}", fact.tool, fact.code))
        .take(8)
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "The last function succeeded and left a real artifact, but completeness warnings remain ({summary}). Do not treat this as a finished edit. Call an allowed function to adjust clips, text, duration, or voiceover, then re-render preview if one already exists. Do not recreate a storyboard that is waiting for confirmation. If the warnings cannot be improved, a later natural-language explanation will be accepted."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn step(tool: &str, result: Value, observation: bool) -> (String, Value, bool) {
        (tool.to_owned(), result, observation)
    }

    #[test]
    fn retryable_write_failure_requests_recovery() {
        let mut state = ContinuationState::default();
        state.record_step(&[step(
            "render_preview",
            json!({"status":"failed","code":"missing_timeline","retryable":true}),
            false,
        )]);
        assert_eq!(state.decide(1, false), Some(ContinuationKind::Recovery));
    }

    #[test]
    fn observation_failure_does_not_request_recovery() {
        let mut state = ContinuationState::default();
        state.record_step(&[step(
            "list_assets",
            json!({"status":"failed","code":"asset_store_unavailable","retryable":true}),
            true,
        )]);
        assert_eq!(state.decide(1, false), None);
    }

    #[test]
    fn non_retryable_or_confirmation_failure_does_not_request_recovery() {
        let mut state = ContinuationState::default();
        state.record_step(&[step(
            "synthesize_voiceover",
            json!({"status":"failed","code":"voice_provider_unconfigured","retryable":false}),
            false,
        )]);
        assert_eq!(state.decide(1, false), None);
        state.record_step(&[step(
            "create_timeline_draft",
            json!({"status":"failed","code":"storyboard_confirmation_required","retryable":true}),
            false,
        )]);
        assert_eq!(state.decide(1, false), None);
    }

    #[test]
    fn quality_warnings_request_refinement_not_recovery() {
        let mut state = ContinuationState::default();
        state.record_step(&[step(
            "replace_text_tracks",
            json!({"status":"ok","qualityWarnings":["cue-1: readability_density"]}),
            false,
        )]);
        assert_eq!(state.decide(2, false), Some(ContinuationKind::Refinement));
        let message = state.take_message(ContinuationKind::Refinement);
        assert!(message.contains("readability_density"));
        assert!(message.contains("finished edit"));
    }

    #[test]
    fn info_preview_checks_do_not_request_refinement() {
        let mut state = ContinuationState::default();
        state.record_step(&[step(
            "render_preview",
            json!({
                "status":"ok",
                "qualityWarnings":[{"category":"black_frames","severity":"info"}]
            }),
            false,
        )]);
        assert_eq!(state.decide(3, false), None);
    }

    #[test]
    fn warning_preview_checks_request_refinement() {
        let mut state = ContinuationState::default();
        state.record_step(&[step(
            "render_preview",
            json!({
                "status":"ok",
                "qualityWarnings":[{"category":"duplicate_footage","severity":"warning"}]
            }),
            false,
        )]);
        assert_eq!(state.decide(3, false), Some(ContinuationKind::Refinement));
    }

    #[test]
    fn recovery_outranks_refinement_in_the_same_step() {
        let mut state = ContinuationState::default();
        state.record_step(&[
            step(
                "replace_text_tracks",
                json!({"status":"ok","qualityWarnings":["cue-1: readability_density"]}),
                false,
            ),
            step(
                "render_preview",
                json!({"status":"failed","code":"missing_timeline","retryable":true}),
                false,
            ),
        ]);
        assert_eq!(state.decide(2, false), Some(ContinuationKind::Recovery));
    }

    #[test]
    fn confirmation_and_last_step_do_not_continue() {
        let mut state = ContinuationState::default();
        state.record_step(&[step(
            "render_preview",
            json!({"status":"failed","code":"missing_timeline","retryable":true}),
            false,
        )]);
        assert_eq!(state.decide(4, true), None);
        assert_eq!(state.decide(MAX_STEPS, false), None);
    }

    #[test]
    fn recovery_budget_then_allows_honest_stop() {
        let mut state = ContinuationState::default();
        state.record_step(&[step(
            "render_preview",
            json!({"status":"failed","code":"invalid_arguments","retryable":true}),
            false,
        )]);
        assert_eq!(state.decide(1, false), Some(ContinuationKind::Recovery));
        let _ = state.take_message(ContinuationKind::Recovery);
        assert_eq!(state.decide(2, false), Some(ContinuationKind::Recovery));
        let _ = state.take_message(ContinuationKind::Recovery);
        assert_eq!(state.decide(3, false), None);
    }

    #[test]
    fn successful_repair_resets_recovery_budget() {
        let mut state = ContinuationState::default();
        state.record_step(&[step(
            "render_preview",
            json!({"status":"failed","code":"invalid_arguments","retryable":true}),
            false,
        )]);
        let _ = state.take_message(ContinuationKind::Recovery);
        state.record_step(&[step(
            "render_preview",
            json!({"status":"ok","qualityWarnings":[]}),
            false,
        )]);
        assert_eq!(state.decide(3, false), None);
        state.record_step(&[step(
            "replace_clips",
            json!({"status":"failed","code":"unavailable_media","retryable":true}),
            false,
        )]);
        assert_eq!(state.decide(4, false), Some(ContinuationKind::Recovery));
    }

    #[test]
    fn repeated_identical_failure_warns_against_same_arguments() {
        let mut state = ContinuationState::default();
        let failure = json!({"status":"failed","code":"invalid_arguments","retryable":true});
        state.record_step(&[step("render_preview", failure.clone(), false)]);
        state.record_step(&[step("render_preview", failure, false)]);
        let message = state.take_message(ContinuationKind::Recovery);
        assert!(message.contains("identical arguments"));
    }

    #[test]
    fn natural_language_without_tools_keeps_open_recovery() {
        let mut state = ContinuationState::default();
        state.record_step(&[step(
            "create_timeline_draft",
            json!({"status":"failed","code":"missing_or_invalid_prerequisite","retryable":true}),
            false,
        )]);
        assert_eq!(state.decide(2, false), Some(ContinuationKind::Recovery));
    }

    #[test]
    fn unrelated_observation_or_prerequisite_does_not_clear_recovery() {
        let mut state = ContinuationState::default();
        state.record_step(&[step(
            "render_preview",
            json!({"status":"failed","code":"missing_timeline","retryable":true}),
            false,
        )]);
        state.record_step(&[step("get_timeline", json!({"status":"ok"}), true)]);
        assert_eq!(state.decide(2, false), Some(ContinuationKind::Recovery));
        state.record_step(&[step(
            "create_timeline_draft",
            json!({"status":"ok","timelineVersionId":"timeline-1"}),
            false,
        )]);
        assert_eq!(state.decide(3, false), Some(ContinuationKind::Recovery));
        state.record_step(&[step(
            "render_preview",
            json!({"status":"ok","qualityWarnings":[]}),
            false,
        )]);
        assert_eq!(state.decide(4, false), None);
    }

    #[test]
    fn adjustment_does_not_clear_warning_until_origin_tool_verifies_it() {
        let mut state = ContinuationState::default();
        state.record_step(&[step(
            "render_preview",
            json!({
                "status":"ok",
                "qualityWarnings":[{"category":"duplicate_footage","severity":"warning"}]
            }),
            false,
        )]);
        state.record_step(&[step(
            "replace_clips",
            json!({"status":"ok","qualityWarnings":[]}),
            false,
        )]);
        assert_eq!(state.decide(3, false), Some(ContinuationKind::Refinement));
        state.record_step(&[step(
            "render_preview",
            json!({"status":"ok","qualityWarnings":[]}),
            false,
        )]);
        assert_eq!(state.decide(4, false), None);
    }
}
