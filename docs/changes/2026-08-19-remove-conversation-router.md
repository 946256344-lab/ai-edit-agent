# 2026-08-19：移除前置对话 Router

NativeToolLoop 现在是生产对话的统一模型入口。`submit_conversation_turn` 在消费 Task Resolver 签发的一次性作用域 receipt 后，所有普通聊天、澄清、项目事实问题和工具执行都创建同一种 Agent task，并直接进入 `run_native_tool_loop`。

移除内容：

- `decide_conversation_route`、`ConversationRouteDecision`、`ConversationRouteResponse`、`InitialAgentSkill`
- 首工具提前选择和 route/goalReasoning/isQuestion/informationScope JSON 协议
- `agentloop/runtime.rs` Legacy JSON decision loop 与显式技能旁路
- `NATIVE_TOOL_LOOP` 环境开关及 Legacy 默认回退路径

保留内容：

- Task Resolver 的 project/editing task/conversation 作用域 receipt
- `RequestToolPolicy` 的用户禁止工具和显式写权限
- 项目事实成功只读观察门、storyboard confirmation 门、取消/10 步/300 秒边界
- `apply_skill`、素材证据、版本/事务、产物真实性和 payload-free 审计

Native 仍使用真实会话 role、统一 Provider ModelTurn、原生 function_call/function_call_output 和 `store:false`；最终回复保存为 assistant。固定 fixture 与契约测试覆盖单入口约束，未调用真实 Provider。

同步文档：`AGENTS.md`、`README.md`、`TASKS.md`、`docs/architecture.md`、`docs/api.md`、`docs/decisions.md`、`docs/harness.md`。
