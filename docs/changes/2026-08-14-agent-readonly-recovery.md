# 2026-08-14 Provider 与 Agent 只读链路恢复

## 目标与边界

在真实 Tauri 桌面应用和既有本地项目中验证 Provider 状态、确定性完成状态查询、Task Resolver、Conversation Router、只读 Agent run、终态回复持久化与刷新恢复。本轮没有创建或修改 storyboard、timeline、preview、Jianying draft、媒体分析任务或最终导出；没有读取、显示或记录 API Key。

## 真实断点

1. UI 与后端均确认自定义 API 已连接、主 Model 已配置，实验性 OAuth 未连接。
2. 首次“剪好了吗？”正确走即时路径，消息增加两条且 Agent task 数保持 2，但错误回答尚无 local preview；真实成果和磁盘中已有 preview。原因是状态函数只解释最近 Agent task 的 `result_json`，没有读取当前 task 的最新真实产物。
3. 首次项目事实问题已在原 task/conversation 完成 `get_storyboard → finish`，没有副作用且 task 为 `completed`；但成功流水线没有调用最终回复持久化，导致确定性回复缺失、conversation 永久 `working`。既有单测只验证了孤立 helper，没有覆盖成功 `finalize_agent_task` 路径。

## 修复

- `get_edit_status` 统一读取上一条 Agent task 运行状态和当前 task 的最新 storyboard、该 storyboard 的最新 timeline 及磁盘实际 preview；当前产物优先于较旧 task result。
- `finalize_agent_task` 在同一 SQLite 事务内写入 task 终态、可选产物审计、确定性最终回复和 conversation 终态，提交后才允许上层发完成事件；失败终态也使用同一事务。
- `initialize_local_store` 恢复仍为 `working`、最新 task 已终态但缺少确定性回复的会话：task 改为 `needs_review`，写入固定恢复消息并将 conversation 标为 `review`；重复启动不会重复消息，也不猜测丢失的模型回答。
- 新增纯状态优先级、成功 finalize 原子回复和启动缺失回复恢复的回归测试。

## 真实桌面验收

- 修复后的“剪好了吗？”：Agent task 数保持 2，消息增加 user/agent 各一条，回复确认已有 local preview，storyboard/timeline/preview ID 未变化，conversation 为 `ready`。
- 历史坏记录在热重载启动后自动变为 `needs_review`，补且仅补 1 条恢复消息，conversation 为 `review`，界面可见。
- 重跑项目事实问题：创建 1 个当前作用域 `agent_loop`；步骤为 `get_storyboard → get_timeline → finish` 且全部完成，操作日志为空；回复准确包含 8 个 storyboard 镜头、8 个 timeline 片段和已存在 preview，确定性完成消息仅 1 条，conversation 为 `ready`，所有产物 ID 未变化。
- 切换成果/Agent 后消息仍可见；WebView 刷新后持久化消息 13 条、Agent 页面可见消息 13 条，最新回复仍仅 1 条，自定义 Provider 仍为 connected。

## 自动验证

- `cargo test --lib edit_status_prefers_current_scoped_artifacts`
- `cargo test --lib finalizing_a_successful_agent_task_persists_its_reply_atomically`
- `cargo test --lib startup_recovery_marks_a_terminal_task_without_a_reply_for_review`
- `cargo fmt --all -- --check`：通过。
- `cargo test`：114 个 Rust 单元测试与 2 个契约测试通过。
- `npm run lint`：通过。
- `npm run build`：通过。
- `npm run harness:check`：通过，触发并满足 `desktop-contract`、`provider-security` 同步规则。
- `git diff --check`：通过，仅有 Git 的 LF/CRLF 工作区提示。

## 同步文档

- `AGENTS.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/roadmap.md`
