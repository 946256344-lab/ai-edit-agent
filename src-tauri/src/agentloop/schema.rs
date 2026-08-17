//! 决策 schema、状态快照与循环状态结构。
//!
//! 定义 Agent 循环中使用的所有数据结构。不包含执行逻辑或提示构建，只提供类型定义。

use crate::models::{
    AgentEditResult, PendingClarificationSnapshot, StoryboardVersion, TimelineVersion,
};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{Duration, Instant};
use tauri::AppHandle;

use super::policy::{LoopGoal, RequestToolPolicy};

/// Maximum number of skill steps the loop will run before stopping.
pub(super) const MAX_STEPS: usize = 10;

/// Timeout for a single agent-loop step model decision.
pub(super) const AGENT_STEP_TIMEOUT: Duration = Duration::from_secs(120);

/// Cooperative budget for model decisions in one interactive Agent run.
pub(super) const AGENT_RUN_TIMEOUT: Duration = Duration::from_secs(300);

/// 会话路由决策：直接回复、澄清或启动 Agent 运行。
#[derive(Debug)]
pub(crate) enum ConversationRouteDecision {
    Respond {
        message: String,
        resolved_clarification_id: Option<String>,
    },
    Clarify(String),
    Run {
        goal: LoopGoal,
        tool: String,
        args: Value,
        project_fact_question: bool,
        resolved_clarification_id: Option<String>,
    },
}

/// 初始技能：路由决策后的首个技能执行。
pub(crate) struct InitialAgentSkill {
    pub(crate) goal: LoopGoal,
    pub(crate) tool: String,
    pub(crate) args: Value,
    pub(crate) project_fact_question: bool,
}

/// 路由响应 schema（内部使用）。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct ConversationRouteResponse {
    pub(super) route: String,
    pub(super) goal: Option<String>,
    pub(super) goal_reasoning: Option<String>,
    pub(super) is_question: Option<bool>,
    pub(super) tool: Option<String>,
    pub(super) answer: Option<String>,
    pub(super) question: Option<String>,
    pub(super) clarification_action: Option<String>,
    pub(super) information_scope: Option<String>,
}

/// 单步决策 schema：模型在每一步返回的决策。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct AgentStep {
    pub(super) goal: Option<String>,
    pub(super) is_question: Option<bool>,
    pub(super) tool: Option<String>,
    #[allow(dead_code)]
    pub(super) reason: Option<String>,
    #[allow(dead_code)]
    pub(super) answer: Option<String>,
    #[allow(dead_code)]
    pub(super) question: Option<String>,
    #[allow(dead_code)]
    pub(super) task_brief: Option<String>,
}

/// Agent 状态快照：提供给模型的紧凑、权威的当前状态视图。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentStateSnapshot {
    pub(super) scope: AgentScopeSnapshot,
    pub(super) assets: AssetAvailabilitySnapshot,
    pub(super) artifacts: ArtifactPresenceSnapshot,
    pub(super) executed_steps: Vec<ExecutedStepSummary>,
    pub(super) remaining_steps: usize,
    pub(super) goal: String,
    pub(super) pending_clarification: Option<PendingClarificationSnapshot>,
    pub(super) unmet_conditions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct AgentScopeSnapshot {
    pub(super) project_id: String,
    pub(super) editing_task_id: String,
    pub(super) conversation_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct AssetAvailabilitySnapshot {
    pub(super) total_count: usize,
    pub(super) usable_count: usize,
    pub(super) pending_analysis_count: usize,
    pub(super) failed_analysis_count: usize,
    pub(super) unavailable_source_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct ArtifactPresenceSnapshot {
    pub(super) storyboard: VersionArtifactSnapshot,
    pub(super) timeline: VersionArtifactSnapshot,
    pub(super) preview: TimelineArtifactSnapshot,
    pub(super) jianying_draft: JianyingArtifactSnapshot,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct VersionArtifactSnapshot {
    pub(super) exists: bool,
    pub(super) version_id: Option<String>,
    pub(super) version_number: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct TimelineArtifactSnapshot {
    pub(super) exists: bool,
    pub(super) timeline_version_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct JianyingArtifactSnapshot {
    pub(super) exists: bool,
    pub(super) timeline_version_id: Option<String>,
    pub(super) registration_status: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(super) struct ExecutedStepSummary {
    pub(super) step_number: usize,
    pub(super) tool: String,
    pub(super) status: String,
    pub(super) produced_artifact: Option<String>,
}

/// Agent 循环状态：包含运行时可变状态的完整上下文。
pub(super) struct LoopState<'a> {
    pub(super) app: &'a AppHandle,
    pub(super) connection: &'a Connection,
    pub(super) agent_task_id: &'a str,
    pub(super) project_id: &'a str,
    pub(super) editing_task_id: &'a str,
    pub(super) conversation_id: &'a str,
    pub(super) task_brief: String,
    pub(super) goal: LoopGoal,
    pub(super) goal_locked: bool,
    pub(super) tool_policy: RequestToolPolicy,
    pub(super) pending_clarification: Option<PendingClarificationSnapshot>,
    pub(super) run_started_at: Instant,
    pub(super) run_deadline: Instant,
    pub(super) history: Vec<(String, String)>,
    pub(super) storyboard: Option<StoryboardVersion>,
    pub(super) timelines: Vec<TimelineVersion>,
    pub(super) last_outcome: Option<AgentEditResult>,
    pub(super) executed_steps: Vec<ExecutedStepSummary>,
    pub(super) last_failed_tool_error_code: Option<&'static str>,
    pub(super) project_fact_question: bool,
    pub(super) successful_observation: bool,
}

impl LoopState<'_> {
    pub(super) fn agent_task_id(&self) -> &str {
        self.agent_task_id
    }
}

/// Agent 循环结果：包含最终产物、状态和目标。
pub(crate) struct AgentLoopResult {
    pub(crate) result: AgentEditResult,
    pub(crate) status: AgentLoopTerminalStatus,
    pub(crate) goal: LoopGoal,
}

/// Agent 循环终止状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AgentLoopTerminalStatus {
    Completed,
    PartiallyCompleted,
    Failed,
    NeedsClarification,
}

impl AgentLoopTerminalStatus {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::PartiallyCompleted => "partially_completed",
            Self::Failed => "failed",
            Self::NeedsClarification => "needs_clarification",
        }
    }
}

/// 循环控制信号：指示循环是否继续、完成或失败。
pub(super) enum AgentLoopControl {
    Continue,
    Done,
    PartiallyDone,
    Failed,
    ExplainedFailure,
    NeedsClarification,
    DeadlineExceeded,
}

/// 从模型响应中提取技能参数，移除元数据键。
pub(super) fn step_args(raw: &Value) -> Value {
    let mut args = raw.clone();
    if let Some(object) = args.as_object_mut() {
        for key in [
            "goal",
            "isQuestion",
            "tool",
            "reason",
            "answer",
            "question",
            "taskBrief",
            "clarificationAction",
            "informationScope",
        ] {
            object.remove(key);
        }
    }
    args
}
