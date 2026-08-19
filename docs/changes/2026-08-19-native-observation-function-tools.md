# 2026-08-19：首批原生观察 Function Tools

## 目标

为 `get_asset_health_summary`、`list_assets`、`get_timeline` 建立集中式原生 Function Tool 定义和 Schema 合约测试。本轮不接入用户请求，不迁移编辑类或副作用工具。

## 变更

- 新增 `src-tauri/src/agentloop/tools.rs`，集中返回三项 Responses 风格 Function Tool 定义。
- 每项包含稳定名称、简洁描述、strict JSON Schema 和 `additionalProperties: false`；严格 schema 将所有属性列入 `required`。
- `get_timeline.timelineVersionId` 语义上可选，通过 `string|null` 允许模型发送 null；定义不包含 projectId、conversationId、本地路径或其他作用域参数。
- 合约测试检查工具名唯一、strict、required 引用完整性、闭合 schema、nullable 参数及现有 `OBSERVATION_TOOLS`/`apply_skill` 路径覆盖。
- 用户请求路径、Provider 注册、Router、LoopGoal、编辑和副作用工具均未修改。

## 同步文档

- `docs/api.md`
- `docs/architecture.md`
- `docs/decisions.md`（新增 ADR-068）
- `docs/codebase/STRUCTURE.md`：补充集中式工具定义模块职责。
- `TASKS.md`

## 验证

使用 Rust 单元测试和静态契约检查；不调用真实 API，不保存凭据、模型原始响应、本机路径或媒体证据。
