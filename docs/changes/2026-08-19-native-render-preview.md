# 2026-08-19：NativeToolLoop 安全接入 render_preview

NativeToolLoop 现在可在用户明确要求“生成预览”时提供原生 `render_preview` Function Tool，并在工具执行后继续请求模型，由模型根据真实结果生成自然语言回复。

- `agentloop/tools.rs` 集中定义严格、闭合的 `render_preview` schema；仅接受可选 nullable 的 `timelineVersionId`，不暴露项目、会话、路径或 FFmpeg 参数。
- `RequestToolPolicy` 在请求发送前过滤“只查看/只检查/不要生成预览”等请求，执行前再次拒绝越权调用。
- Rust 从当前项目作用域选择时间线并复用 `apply_skill`；成功返回脱敏的 preview 产物收据，失败返回安全错误码和可恢复提示。
- 固定 JSON fixture 覆盖成功回合、只读过滤和无时间线失败后的模型回复；Legacy Runtime、Router、LoopGoal 和确认门未迁移。
- Native 开关下的显式“生成预览”进入原生 loop；其他显式 Legacy 命令保持原路径。执行边界复核同一份正向授权，问句和解释性请求不获得预览工具。
- 明确预览请求有真实完成门：只有 `function_call_output` 含 `status=ok` 和 `artifact.type=preview` 才算成功；失败结果仍交模型解释，但任务不会被标为完成。
- preview 已验证但模型总结请求失败时，任务保留真实产物并标记 `partially_completed`，使用诚实恢复文案，不回退为“未生成”。
- 后续回归修复：大型只读工具结果超过输入预算时，裁剪逻辑保留最新 `function_call` 与对应 `function_call_output`，避免模型看不到观察结果而重复调用工具并耗尽步骤；旧历史仍按预算淘汰。

同步文档：

- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/codebase/STRUCTURE.md`
- `docs/harness.md`
- `README.md`
- `TASKS.md`
- `AGENTS.md`
