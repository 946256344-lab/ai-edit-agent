# 修复会话隔离漏洞：消息历史按 editing_task 过滤

**日期**：2026-08-20  
**类型**：bugfix (security/privacy)  
**影响范围**：Agent 上下文加载、会话隔离、用户隐私

## 问题

用户报告严重的会话隔离问题：Agent 识别到其他会话的内容（例如在新会话中提到其他会话的 "Agnes"），导致跨会话数据泄漏。

**根本原因**：`agentloop/prompt.rs::load_native_message_history` 查询只按 `conversation_id` 过滤消息，没有验证消息所属的 `editing_task_id`。由于 `conversations` 表有 `editing_task_id` 外键但 `messages` 表没有直接外键，当多个 editing_task 共享同一个 conversation_id 时（理论上不应该发生，但数据库约束未阻止），所有这些会话的消息会被混合加载。

**影响**：
- 严重的用户隐私问题：用户 A 的会话可能看到用户 B 的消息
- 违反"我们需要每个会话严格隔离"的核心产品需求
- 如果 conversation 被错误关联到多个 editing_task，Agent 会产生混乱的上下文

## 解决方案

在 `load_native_message_history` 查询中添加 JOIN 和 `editing_task_id` 过滤，确保只加载属于当前 editing_task 的消息。

### 变更细节

**src-tauri/src/agentloop/prompt.rs**：

1. **函数签名变更**：`load_native_message_history` 新增 `editing_task_id: &str` 参数
2. **查询变更**：
   ```sql
   -- 旧查询（不安全）
   SELECT role, content FROM messages
   WHERE conversation_id = ?1 AND role IN ('user', 'assistant', 'agent')
   ORDER BY created_at DESC, id DESC LIMIT ?2
   
   -- 新查询（安全）
   SELECT messages.role, messages.content FROM messages
   JOIN conversations ON conversations.id = messages.conversation_id
   WHERE messages.conversation_id = ?1
     AND conversations.editing_task_id = ?2
     AND messages.role IN ('user', 'assistant', 'agent')
   ORDER BY messages.created_at DESC, messages.id DESC LIMIT ?3
   ```
3. **测试更新**：单元测试 `load_agent_context_excludes_current_user_request_and_reverses_order` 传入 `editing_task_id = 't'`

**src-tauri/src/agentloop/native.rs**：

- 调用点更新：`load_native_message_history(connection, conversation_id, editing_task_id, request)`

### 为什么不直接在 messages 表添加 editing_task_id？

当前架构中：
- `messages` 属于 `conversations`（通过 `conversation_id` 外键）
- `conversations` 属于 `editing_tasks`（通过 `editing_task_id` 外键）
- 层级关系已明确：`messages → conversations → editing_tasks`

添加 `messages.editing_task_id` 会引入数据冗余和不一致风险（如果 conversation 的 editing_task_id 改变，需要同步更新所有关联消息）。通过 JOIN 查询保持单一事实源，避免迁移现有数据。

## 验证

- **Rust 库测试**：163 个测试通过，包括 `agentloop::prompt` 的会话历史单元测试
- **前端 lint/build**：通过
- **架构与契约检查**：`harness:test` 通过

## 契约影响

- **Rust 内部 API**：`load_native_message_history` 签名变更（仅在 `agentloop` 模块内部使用）
- **Tauri 命令**：无变更
- **SQLite schema**：无变更（利用现有 `conversations.editing_task_id` 外键）
- **前端接口**：无变更

## 安全性

**修复前**：如果数据库约束或应用逻辑错误导致多个 editing_task 关联同一 conversation，会发生跨会话数据泄漏。

**修复后**：即使存在数据异常，Agent 历史加载严格按 `editing_task_id` 隔离，确保会话之间不会互相看到对方的消息。

## 后续建议

1. **数据库约束增强**：考虑在 `conversations` 表添加 UNIQUE 约束或业务逻辑验证，确保一个 conversation 只能属于一个 editing_task
2. **审计现有数据**：检查是否存在 conversation 被错误关联到多个 editing_task 的历史数据
3. **集成测试**：添加端到端测试验证多会话场景下的隔离性

## 同步文档

- [x] `docs/architecture.md` - 会话隔离与消息层级关系
- [x] `docs/api.md` - `agentloop::prompt` 内部函数签名（如有公开）
- [x] `TASKS.md` - 任务窗口记录
- [x] `AGENTS.md` - 无需变更（代码结构规则未变）
- [x] `docs/decisions.md` - 无需新 ADR（修复现有架构意图）
