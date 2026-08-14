# 2026-08-07：将请求决策彻底统一为单一目标驱动技能循环

## 变更

- `src-tauri/src/models.rs`：删除 `AgentEditDecision` 枚举、`AgentEditCommon` 结构体及其 `EMPTY_COMMON`/`impl`；`AgentEditResult` 保持不变，仍是循环与显式命令的通用产物载体。
- `src-tauri/src/agent.rs`：删除 `ToolDecisionProvider`/`ModelToolDecisionProvider`/`request_agent_edit_decision`/`retarget_decision_for_draft`/`verified_action_message`/`safe_follow_up_reply`，不再让模型选工具，也不再有“已识别快速路径 + Unknown 升级”分叉。新增 `explicit_command_tool`（精确匹配“创建剪映草稿/创建内部时间线/生成预览”等单命令）走 `run_explicit_command` 确定性直通路径；其余所有请求直接进入 `run_agent_loop`。新增 `finalize_agent_task`：成功时写 `completed`/结果摘要并按产物落地调用 `record_agent_operation`，失败时用 `safe_tool_failure` 记安全失败码并返回固定诚实文案，不再请求模型编造后续回复。
- `src-tauri/src/agentloop.rs`：改为目标驱动、封闭、有界的技能循环。新增 `LoopGoal`（问答/storyboard/内部时间线/preview/剪映草稿）、`derive_loop_goal`、`goal.satisfied_by` 完成门、`honest_no_change`/`corrective_message`、`run_explicit_command`、`step_args`（剔除 `tool`/`reason`/`answer`/`question`/`taskBrief` 元字段）、`apply_skill`（复用观察与编辑/交付技能）、`select_timeline_for_tool`/`ensure_timeline` 等。`finish`/`no_action`/`done` 只有在目标产物真实存在时才结束，否则回纠偏消息并继续直至步数上限（6）；技能失败只回读错误，绝不自动无限重试；终端回复由真实产物组装，无产物时使用固定诚实文案。
- 移除了 3 项依赖认证 Provider 的集成测试（`experimental_agent_*`）；新增 6 项 `agentloop.rs` 单测。

## 同步文档

本变更同步了以下长期文档：`AGENTS.md`、`TASKS.md`、`docs/architecture.md`、`docs/api.md`、`docs/decisions.md`（新增 ADR-032，并将 ADR-031 标记为被取代）。

## 验证

- `cargo build --lib` 编译通过。
- `cargo test --lib` 19 通过、0 失败（含 agentloop 新增 6 项、agent 保留的 2 项）。
- `npm run lint`（0 warn/0 err）与 `npm run build` 通过。
- 待真实 Provider 响应验证循环目标判定、完成门与产物落地消息。前端契约（`agent-tools.ts`、`App.tsx` 的 `agent-edit-completed`/`applyPreview`）未改动，`execute_agent_edit` 命令签名字段保持不变。