# 2026-08-20：清理 Legacy Runtime

NativeToolLoop 已是普通聊天、连续追问、项目事实和工具执行的唯一对话 Runtime。本次在生产引用审计后删除剩余回退面，不改变领域算法或安全边界。

## 删除

- 无调用者的 `execute_agent_edit` Tauri 命令、TypeScript wrapper 与 API 契约。
- Router `immediate` 返回变体和前端不可达分支。
- `agent.rs` 创建 Router 来源 pending clarification 的能力；新记录固定绑定 `agent_run`。
- 执行卡中的 `finish`、`done`、`no_action` 标签和未使用的 clarification 快照类型。
- Provider helper 名称中的 decision 语义；仍被 Task Resolver、storyboard 和视觉任务使用的通用提取器改名为 `model_response_json_object_text`。

## 保留

- Task Resolver 与一次性作用域 receipt。
- RequestToolPolicy、用户禁止工具、确认门、项目事实观察门、超时/步骤上限、取消和审计。
- `apply_skill`、素材证据、源时间范围、版本、事务与产物真实性校验。
- SQLite 对历史 `router` clarification 和 `agent` 角色消息的读取兼容。

## 验证

最终静态测试、桌面验收、运行收据和已知限制在本任务完成后补入本节。

## 同步文档

- `AGENTS.md`
- `README.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/harness.md`
- `docs/roadmap.md`
- `docs/changes/2026-08-20-cleanup-legacy-runtime.md`
