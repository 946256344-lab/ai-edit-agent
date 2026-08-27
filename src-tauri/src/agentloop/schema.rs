//! Native Agent Loop 的运行状态与终态结构。
//!
//! 这里只保存 Rust 执行边界需要的作用域、产物缓存和运行预算；模型不再提交 route、
//! goal 或首工具决策协议。

use crate::models::{AgentEditResult, StoryboardVersion, TimelineVersion};
use rusqlite::Connection;
use std::time::Duration;
use tauri::AppHandle;

use super::policy::RequestToolPolicy;

/// 单次 Native 循环最多允许的模型步骤数。
pub(super) const MAX_STEPS: usize = 10;

/// 单次模型请求的硬超时。
pub(super) const AGENT_STEP_TIMEOUT: Duration = Duration::from_secs(120);

/// 一轮用户请求的总模型决策预算。
pub(super) const AGENT_RUN_TIMEOUT: Duration = Duration::from_secs(300);

/// Native 工具执行共享同一作用域和产物缓存；模型文字不能修改这些事实。
pub(super) struct LoopState<'a> {
    pub(super) app: &'a AppHandle,
    pub(super) connection: &'a Connection,
    pub(super) agent_task_id: &'a str,
    pub(super) project_id: &'a str,
    pub(super) editing_task_id: &'a str,
    pub(super) conversation_id: &'a str,
    pub(super) task_brief: String,
    pub(super) tool_policy: RequestToolPolicy,
    pub(super) storyboard: Option<StoryboardVersion>,
    pub(super) timelines: Vec<TimelineVersion>,
    pub(super) last_outcome: Option<AgentEditResult>,
    pub(super) last_failed_tool_error_code: Option<&'static str>,
    pub(super) successful_observation: bool,
}

impl LoopState<'_> {
    pub(super) fn agent_task_id(&self) -> &str {
        self.agent_task_id
    }
}

/// Native 循环输出；澄清恢复状态由结构化字段携带，不依赖模型目标声明。
pub(crate) struct AgentLoopResult {
    pub(crate) result: AgentEditResult,
    pub(crate) status: AgentLoopTerminalStatus,
    pub(crate) clarification_goal: Option<&'static str>,
}

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
