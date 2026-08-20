# Provider 原生工具调用数据结构

## 目标

为 OAuth Responses 和自定义 Chat Completions Provider 建立可替换的统一模型轮次结构，同时保持旧 JSON decision 解析路径不变。本轮不接入 Router、LoopGoal 或 Agent Runtime。

## 触发规则

- desktop-contract（修改 `src-tauri/src/provider.rs`）
- provider-security（修改 `src-tauri/src/provider.rs`）

## 变更

- `provider.rs` 新增 `ModelTurn`、`ModelOutputItem` 与 `FunctionCall`，保留完整 Responses `output` 项（包括已识别函数调用的 raw item）并归一化 message/function call。
- Responses 普通 JSON、Responses SSE 完成 output/item 事件、Chat Completions 普通响应和 SSE tool-call 增量均可解析为统一结构。
- Responses 风格输入转换到 Chat Completions 时保留 `tools`、`tool_choice`、`parallel_tool_calls`、`stream`；并行函数调用合并到同一条 assistant `tool_calls` 消息，函数结果映射为 `tool` + `tool_call_id`，对象结果序列化为 JSON 字符串。
- `store: false` 不会影响 Responses output item 的解析；自定义 Chat 请求不发送 Responses 专属 `store` 字段。
- 固定 fixture 覆盖普通响应、SSE 增量、工具请求和完整 output；fixture 不含凭据、绝对路径或原始敏感响应。

## 边界

Legacy Runtime 继续使用 `model_response_json_text`，统一 `ModelTurn` 尚未驱动 Agent 工具循环。没有修改 Router、LoopGoal、工具白名单、SQLite schema 或副作用确认门。

## 同步文档

- `AGENTS.md`：补充 Provider 统一解析边界的维护记录。
- `docs/architecture.md`：补充 Provider 原生工具调用适配与 Legacy Runtime 边界。
- `docs/api.md`：补充请求字段透传、统一解析类型与 `store:false` 语义。
- `docs/decisions.md`：新增 ADR-067。
- `TASKS.md`：登记本轮唯一任务并保留完成门记录。
- `README.md`：补充 Provider 边界维护记录。
- `docs/harness.md`：记录本轮文档同步规则覆盖范围。
