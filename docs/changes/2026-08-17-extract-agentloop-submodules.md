# 2026-08-17：拆分 agentloop.rs 为四个子模块

## 变更类型

重构（refactor）

## 动机

`agentloop.rs` 已达到 3684 行，职责混杂：路由决策、提示构建、技能执行器和纯类型定义全部堆在同一文件。按职责层次拆分为子模块，使每层边界清晰、测试可直接访问目标函数，架构预算棘轮可独立守护每层。

## 变更内容

### 后端 Rust

1. **新增 `src-tauri/src/agentloop/schema.rs`（245 行）**：
   - 纯类型定义与常量：`MAX_STEPS`、`AGENT_STEP_TIMEOUT`、`AGENT_RUN_TIMEOUT`
   - 所有循环内结构体：`LoopState<'a>`、`AgentStateSnapshot`、`ArtifactPresenceSnapshot` 等
   - `AgentLoopControl` 枚举、`AgentLoopResult`、`AgentLoopTerminalStatus`、`InitialAgentSkill`
   - 无对兄弟模块的依赖

2. **新增 `src-tauri/src/agentloop/prompt.rs`（~500 行）**：
   - 提示构建：`build_step_prompt`、`build_agent_state_snapshot`
   - 历史加载：`load_message_history`、`render_history`
   - 状态快照辅助：`load_asset_availability`、`current_artifact_presence`
   - 前置条件提示：`unmet_conditions`、`deterministic_prerequisite_hints`
   - 公开契约：`load_pending_clarification`（`pub(crate)`）

3. **新增 `src-tauri/src/agentloop/skills.rs`（~920 行）**：
   - 技能执行器：`apply_skill`（分发 20+ 技能）
   - 时间线辅助：`select_timeline_for_tool`、`build_timeline_snapshot`
   - 状态辅助：`read_scoped_edit_status`（`pub(crate)`）、`edit_status_message`
   - 错误安全辅助：`safe_tool_failure_context`、`safe_step_error_code`

4. **新增 `src-tauri/src/agentloop/runtime.rs`（~1360 行）**：
   - 会话路由决策：`decide_conversation_route`（`pub(crate)`）
   - 主循环：`run_agent_loop`、`run_agent_loop_with_initial_skill`（`pub(crate)`）
   - 显式命令直通：`run_explicit_command`（`pub(crate)`）
   - 终态构建：`finalize_result`、`finalize_terminal`
   - 内部辅助：`run_step`、`reject_user_restricted_tool`、`execute_initial_skill`

5. **重写 `src-tauri/src/agentloop.rs`（薄层，仅 re-export 与测试）**：
   - 声明四个子模块：`schema`、`prompt`、`skills`、`runtime`
   - `pub(crate)` re-export 外部所需类型与函数
   - 保留全部 103 个测试（通过 `pub(super)` 访问精确测试目标）

### 架构检查器

6. **修改 `scripts/check-agent-contracts.mjs`**：
   - 扩展控制动作扫描：将 `agentloop/runtime.rs` 加入 `runtimeControlNames` 的输入，以定位已迁入该文件的 canonical `matches!(tool.as_str(), ...)` 表达式
   - 不影响其他规则

## 不变边界

- 公开 Tauri 命令名称、参数、返回值
- SQLite schema
- Agent 工具白名单
- Provider 接口
- 用户数据

## 同步文档

- AGENTS.md — 维护记录：agentloop 拆分与检查器扩展
- README.md — 维护记录：agentloop 分层重构
- docs/architecture.md — 无内容变化（已在同批改动中）
- docs/api.md — 无内容变化（已在同批改动中）
- docs/decisions.md — ADR-066：agentloop.rs 分层拆分为四个子模块
- docs/harness.md — 维护记录：check-agent-contracts.mjs 扩展扫描 runtime.rs
- TASKS.md — 任务完成标记

## 完成门

- 103 个 Rust 库测试通过（含提取后新增的路由/提示/技能精确测试）
- `cargo fmt --check` 通过
- `npm run lint` 与 `npm run build` 通过
- `npm run harness:test` 与 `npm run harness:check` 通过
