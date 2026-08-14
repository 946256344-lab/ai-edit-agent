# Conversation Router

## 变更

- 新增 `submit_conversation_turn`，把即时回复/澄清与异步 Agent run 分开。
- 精确状态问题直接读取同作用域上一条 Agent run，不调用模型。
- 首轮模型响应同时决定 `respond`、`clarify` 或 `run`，执行型首个工具注入既有 loop 作为 step 1。
- 前端改用判别式 `ConversationTurnResult`；只有 `run` 接收 `agent-edit-completed`。
- 保留 `execute_agent_edit` 兼容入口和现有工具完成门。
- schema v7 新增结构化 `pending_clarifications`，覆盖即时 router 澄清和异步 `ask_user`；待澄清会进入 Agent 状态快照，并以 `keep/resolved/superseded` 生命周期跨重启保留。
- 执行型 task 创建与旧澄清解决、异步待澄清任务终态与问题写入均以 SQLite 事务提交。

## 未完成

- 完整的 `ConversationRouterSnapshot` 公开结果尚未加入。
- 真实 Provider 与桌面端即时/异步竞态仍需手工验证。

## 验证

- `cargo test`：87 个库测试、2 个契约测试通过。
- `npm run lint`、`npm run build`、`npm run harness:check` 通过。

## 同步文档

- `AGENTS.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
