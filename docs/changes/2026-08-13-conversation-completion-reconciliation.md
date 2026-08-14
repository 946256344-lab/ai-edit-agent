# 对话完成持久化与终态对账

## 问题

- 实际 Agent run 已在 SQLite 中完成，但执行卡只轮询步骤，父级仍持有旧的 `running` 任务快照，计时器会持续增长。
- 最终 Agent 回复原先由前端收到 `agent-edit-completed` 后再写入 conversation；事件丢失时不仅 UI 不停止，回复本身也无法从持久化状态恢复。
- 项目事实问答在 `list_assets` 已包含所问数量后，仍可能调用语义重叠的健康汇总工具，再多消耗一次模型往返。
- `finish` 没有用户文案映射，被错误展示成“执行受限操作”。

## 变更

- 后端在 Agent run 终态后、发事件前，以 `agent-task-result-{agentTaskId}` 为确定性消息 ID 幂等写入最终回复；重复通知不会重复消息。
- 完成写入校验原 project、editing task 与 conversation；只有不存在更新用户请求或更新活动任务时才把 conversation 恢复为 `ready`。
- 前端保留最多 20 个早到事件，同时在发送中或当前任务活跃时每 1.2 秒读取 `list_agent_tasks`。事件或轮询任一方发现终态后，都会按原作用域重载消息、storyboard、时间线、preview、任务和操作审计。
- 完成对账以任务 ID 去重，正在对账与已对账集合分离，已对账集合限制为 100 条。
- 项目事实问答提示增加收敛规则：最新成功观察已包含所问数量、状态或事实时直接 `finish`；只有明确缺少所问事实时才继续观察。
- 执行卡把 `finish`/`done` 映射为“整理并回答”，把 `no_action` 映射为“确认无需操作”。

## 安全与边界

- 没有新增 Tauri 命令、依赖或持久化表；消息继续只保存在本机 SQLite。
- `agent-edit-completed` 契约字段不变，但从唯一交付通道降级为低延迟通知。
- 前端只有在原项目和剪辑任务仍是活动作用域时更新可见产物；跨作用域结果只保留在原 conversation。
- 不增加关键词业务直通分支；模型仍选择项目事实问题的首个观察工具。
- 日志仍不记录模型原文、会话内容、媒体证据、凭据或本机路径。

## 验证

- `cargo test --lib agent_completion_message_is_idempotent_and_marks_conversation_ready`
- `cargo test --lib grounded_project_question_finishes_without_redundant_confirmation`
- `npm run lint`
- `npm run build`
- 完整 Rust、harness 与文档一致性检查在最终验证阶段执行。

## 同步文档

- `AGENTS.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/roadmap.md`
