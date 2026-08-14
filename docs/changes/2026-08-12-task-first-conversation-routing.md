# Task-first conversation routing

## 变更

- schema 升至 v10，新增 `task_state_snapshots`、`pending_task_routes` 与严格绑定目标 conversation/唯一 user message 的 `task_route_receipts`。
- 新增 `resolve_conversation_task`，在当前 local project 内返回继续当前任务、切换已有任务、创建新任务或澄清。
- 任务候选只包含受限目标、当前子目标、真实产物阶段/标识、完成项与安全状态；不把会话侧栏摘要或完整聊天历史当任务记忆。
- 任何模型自动归属都使用 0.85 置信度门，模型不能自报只读来降低门槛；未知或跨项目 task ID 会被拒绝。
- 新任务与 conversation 在路由事务内原子创建；每个确定性路由签发绑定确切项目、task、conversation 与完整请求的一次性凭证。
- 前端改为先解析任务，再激活目标会话、加载其 storyboard/时间线，最后保存消息并携带 route receipt 进入 Conversation Router；后端提交入口和兼容执行入口都强制消费凭证。
- `create_message(role=user)` 也必须校验未消费凭证，阻止绕过路由直接污染其他 conversation；提交失败会把 conversation 从 `working` 恢复为 `ready`。
- Agent run 完成后重建任务事实快照；任务收到已归属请求时更新受限当前子目标。

## 安全与兼容性

- Task Resolver 不选择工具、不执行副作用，也不能跨 local project 路由。
- 目标任务未确定时不向任一 conversation 写入用户消息；路由澄清保留在项目级结构化状态。
- `pendingAction=keep` 保留原待归属请求；pending 只在绑定凭证被提交入口成功消费时 resolved，凭证不可重复使用。
- 同一 pending 若并发签发多个凭证，胜出者消费时事务内删除 sibling；落败凭证既不能执行，也不能预先写入其他 conversation。
- 精确单命令仅在没有待路由澄清时使用用户当前显式选择的任务。
- 不记录模型响应原文、媒体证据、凭据或本机路径。
- 既有 Conversation Router、Agent loop、真实产物完成门、Jianying 单向交付和最终导出确认边界不变。

## 验证

- `cargo test`：96 个库测试与 2 个契约测试通过（新增 pending keep、凭证单次消费、延迟 resolved、create-new 精确绑定、同 task 跨 conversation 拒绝和 pending 并发消费测试）。
- `npm run lint`：通过。
- `npm run build`：通过。
- 真实桌面 Provider 的跨任务自然语言路由仍待手工验证。

## 同步文档

- `AGENTS.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/roadmap.md`
