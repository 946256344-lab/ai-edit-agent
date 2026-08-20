# 2026-08-19：NativeToolLoop 使用原生会话消息

## 目标

让 NativeToolLoop 从 SQLite 读取真实 user/assistant 会话消息，并以原生 item 继续工具调用循环；不再把历史拼成带说话人标签的 Prompt。

## 变更

- 新增原生会话历史 loader：按 `created_at`/`id` 顺序读取当前 conversation 的 user、assistant（兼容旧 agent）消息，保留真实 role/content，当前请求只排除一次。
- NativeToolLoop 增加上下文预算；只删除旧消息，当前请求以及 `function_call`/`function_call_output` 对保持完整。
- Native 成功回复在同一终态事务中以 `assistant` 角色保存；Legacy Runtime 继续保存 `agent`。
- SQLite schema 升至 v15，迁移旧 `messages` 表的 role 约束并保留已有行；前端 StoredMessage 联合类型同步接受 assistant。
- system 内容仅保留身份、只读安全边界和项目事实观察约束；项目状态仍由三项只读工具读取。

## 边界

不持久化原始模型响应、工具 transcript、凭据、本地路径或媒体证据；不修改 Router、LoopGoal、Legacy Runtime、工具白名单或副作用权限。

## 验证

固定 SQLite/Provider fixture 覆盖真实 role、历史截断、工具调用配对、assistant 持久化和旧 schema 迁移；`cargo test --lib`、前端 lint/build、agent/harness 检查通过后再提交。

## 同步文档

- `TASKS.md`
- `src/lib/local-store.ts`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/changes/2026-08-19-native-readonly-agent-loop.md`
