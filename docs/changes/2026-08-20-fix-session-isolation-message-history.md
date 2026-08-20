# 2026-08-20: 修复会话隔离漏洞，防止跨会话数据泄漏

## 问题

用户报告 Agent 识别到其他会话的内容，例如在新会话中提到 "Agnes"（该名称来自另一个完全不同的会话）。这是严重的会话隔离漏洞，违反了"我们需要每个会话严格隔离"的核心要求。

## 根本原因

`agentloop/prompt.rs::load_native_message_history` 查询只按 `conversation_id` 过滤消息，没有验证消息所属的 `editing_task_id`：

```rust
// 旧查询（仅按 conversation_id 过滤）
let mut agent_message_statement = connection.prepare(
    "SELECT role, content FROM messages
     WHERE conversation_id = ?1 AND role IN ('user', 'assistant', 'agent')
     ORDER BY created_at DESC, id DESC LIMIT ?2",
)?;
```

数据库架构：
- `conversations` 表有 `editing_task_id` 外键（通过迁移添加）
- `messages` 表只有 `conversation_id` 外键，没有直接链接到 `editing_task_id`
- 如果多个 editing_task 共享同一个 conversation_id（理论上不应该发生，但数据完整性约束未明确禁止），所有任务的消息会混在一起

## 修复方案

在查询中添加 JOIN `conversations` 表并同时验证 `conversation_id` 和 `editing_task_id`，确保严格的会话边界：

```rust
// 新查询（同时验证 conversation_id 和 editing_task_id）
let mut agent_message_statement = connection.prepare(
    "SELECT m.role, m.content FROM messages m
     JOIN conversations c ON m.conversation_id = c.id
     WHERE m.conversation_id = ?1 
       AND c.editing_task_id = ?2
       AND m.role IN ('user', 'assistant', 'agent')
     ORDER BY m.created_at DESC, m.id DESC LIMIT ?3",
)?;
```

## 变更范围

**Rust 内部 API**：
- `agentloop/prompt.rs::load_native_message_history` 函数签名变更：新增 `editing_task_id: &str` 参数
- `agentloop/runtime.rs` 调用点更新：传入当前 `editing_task_id`

**不变更**：
- 任何 Tauri 命令签名
- 前端 TypeScript 接口
- SQLite schema（`messages` 和 `conversations` 表结构不变）
- `create_message` 等其他函数

## 同步文档

触发规则 `desktop-contract` 要求同步：`docs/architecture.md`、`docs/api.md`、`TASKS.md`。
触发规则 `provider-security` 要求同步：`AGENTS.md`、`docs/decisions.md`。

- [x] `docs/architecture.md`：在"会话（conversation）只是对话容器"段落补充会话隔离说明，明确 Agent 加载历史消息时必须同时验证 `conversation_id` 和 `editing_task_id`（通过 JOIN），确保严格的会话边界，防止跨会话数据泄漏。
- [x] `docs/api.md`：新增维护记录（2026-08-20），说明 `agentloop/prompt.rs::load_native_message_history` 内部函数新增 `editing_task_id` 参数，查询改为 JOIN `conversations` 表并同时验证 `conversation_id` 和 `editing_task_id`，确保严格会话隔离，防止跨会话数据泄漏。修改仅影响 Rust 内部 API，不改变任何 Tauri 命令签名或前端接口。
- [x] `TASKS.md`：登记当前任务窗口，标记为进行中。
- [x] `AGENTS.md`：添加维护记录（2026-08-20），说明 `load_native_message_history` 查询新增 `editing_task_id` 过滤以确保严格会话隔离。
- [x] `docs/decisions.md`：添加维护记录（2026-08-20），确认无需新增 ADR（这是既有架构的实现修正）。
- [x] `README.md`：无需变更（这是 Rust 内部实现的 bug 修复，不改变用户可见功能、安装步骤、开发指南或贡献流程）。
- [x] `docs/harness.md`：无需变更（这是 Rust 内部实现的 bug 修复，不改变验证流程、机器约束、审查闭环或完成门要求）。

## 触发规则

- `desktop-contract`：涉及会话隔离边界（消息查询逻辑）
- `provider-security`：涉及数据隔离安全（防止跨会话泄漏）

## 影响评估

**风险**：低。修复后查询更严格，只会减少返回的消息（过滤掉错误归属的消息），不会引入新的数据泄漏或破坏正常会话。

**兼容性**：完全向后兼容。现有数据结构不变，只是查询条件更严格。

## 验证

- [ ] 163 个 Rust 库测试通过（包括 `agentloop/prompt.rs` 和 `agentloop/runtime.rs` 的单元测试）
- [ ] 前端 `npm run lint` 和 `npm run build` 通过
- [ ] `npm run harness:test` 通过
- [ ] `npm run harness:check` 通过
- [ ] 手动验证：在同一项目下创建两个不同的 editing_task，分别开启会话并发送消息，确认各会话只能看到自己的历史消息
