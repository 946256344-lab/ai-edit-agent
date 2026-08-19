# 2026-08-19：NativeToolLoop 只读原生 Agent Loop

## 目标

在显式 `NATIVE_TOOL_LOOP` 开关下验证原生 Function Tool 的只读对话循环；Legacy Runtime 继续默认启用。本轮只允许 `get_asset_health_summary`、`list_assets`、`get_timeline`。

## 变更

- 新增 `agentloop/native.rs`，直接消费统一 `ModelTurn`，不经过 `decide_conversation_route`，不要求 JSON decision，也不使用 Legacy 的控制动作。
- 请求固定 `store:false`、`parallel_tool_calls:false`，携带三项集中式 strict tools；下一轮追加完整 Responses output item（Chat Provider 转换为等价 assistant/function call 项）和 `function_call_output`。
- 工具执行复用现有 `apply_skill`。未知工具、参数错误和技能失败只产生脱敏结构化错误；模型仍有机会解释或调整。
- 保留 10 步上限、300 秒总预算、每步 120 秒超时和 agent task cancelled 检查；显式命令、编辑工具、副作用工具、确认门和产物真实性校验未迁移。
- 新增固定 JSON fixture，覆盖普通问候、`list_assets`、`get_timeline`、安全失败恢复及开关默认值。

## 安全边界

不记录凭据、原始模型响应、用户内容或本地绝对路径；诊断只记录响应字节数和固定审计错误码。fixture 不调用真实 API。

## 验证

`cargo test --manifest-path src-tauri/Cargo.toml`（128 个库测试 + 2 个契约测试通过），随后执行 `agent:check`、`harness:test`、`harness:check` 及完成门命令。

## 同步文档

- `AGENTS.md`
- `README.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/harness.md`
- `docs/codebase/STRUCTURE.md`
