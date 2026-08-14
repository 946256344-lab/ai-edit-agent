# 2026-08-07：Agent 编辑异步派发

## 变更

- `execute_agent_edit` 由阻塞命令改为异步派发：命令先同步校验、插入 `queued` 调用并立即返回任务 ID，完整流水线（模型决策、作用域校验、工具副作用与审计）在后台线程执行，终态经 `agent-edit-completed` 事件携 `AgentEditResult` 回传。
- 保留工具白名单（`generate_storyboard`、`create_timeline_draft`、`replace_timeline_clip`、`render_preview`、`create_jianying_draft`、`no_action`）、项目/任务/会话作用域校验、副作用审计与 `needs_review` 恢复策略。
- 命令返回类型由 `AgentEditResult` 改为 `String`（任务 ID）。事件 `status` 由持久化的 `agent_tasks.status`（`completed`/`failed`）判定。
- 前端 `src/lib/local-store.ts` 的 `executeAgentEdit` 改为返回 `Promise<string>`；`App.tsx` 新增 `agent-edit-completed` 事件监听，根据事件结果应用 storyboard/时间线/preview、追加回复、轮询任务状态并恢复会话 `ready`。
- 新增 `models::AgentEditEvent`（`agentTaskId`、`status`、`result`）。

## 验证

- `cargo build`、`cargo test`（13 通过、3 依赖实验性 Provider 的集成测试按设计跳过）通过。
- `npm run lint`、`npm run build`、`npm run harness:check` 通过。
- 未改动既有 Tauri 命令名与入参；`execute_agent_edit` 仍为唯一入口。

## 遗留 TODO

- 可恢复的本地 Agent 运行时（并发队列、暂停/恢复）尚未实现；本次仅完成异步化。
- 事件驱动结果依赖前端监听 `agent-edit-completed`，需在 Tauri 桌面应用内以真实素材与实验性 Provider 手工验收。