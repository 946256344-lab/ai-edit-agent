# 移除 NativeToolLoop 固定单目标限制

## 目标

Native 对话不再为整轮请求声明或锁定唯一 LoopGoal。模型可在同一轮按原生 function_call 跨越多个已授权工具；无 function_call 且有自然语言时结束。任务终态继续由 Rust 的真实工具收据和持久化产物裁决。

## 变更

- `agentloop/native.rs`：移除 preview/写请求的固定目标纠偏；RunReceipt 只跟踪实际执行的具名成功、排队和未恢复失败，同一工具修正成功可清除旧失败。超时和步骤上限保留累计的已验证中间产物，没有成功工具则失败。自然语言可以结束循环，但未执行工具的完成声明不能产生 completed 或 artifact。
- `agentloop/native_policy.rs` 与 `policy.rs`：复合请求可同时获得 storyboard、timeline、text、preview 的明确授权；解释性请求仍保持只读。发送前过滤和执行前二次校验保持不变。
- 固定 Responses fixture 覆盖“检查素材，做 30 秒剪辑，加字幕并生成预览。”先调用 `list_assets`、`generate_storyboard` 并进入真实 `needs_confirmation` 边界；有效确认后的下一轮依次调用 `create_timeline_draft`、`replace_text_tracks`、`render_preview`，最后返回自然语言。
- 契约检查器删除旧控制动作对账，改为禁止 `LoopGoal`、目标锁以及 `finish`/`done`/`no_action` 伪控制动作回流 Native 生产路径。历史回归 fixture 以 `assistantReply` 表达自然语言结束。
- storyboard 的 `needs_confirmation` 继续阻止后续非观察写工具；本变更不绕过确认、作用域、素材证据、版本、事务、审计、许可证、文字能力或 Jianying 兼容性校验。
- 独立审查发现并关闭：复合否定句误授权、不可达的 storyboard `ok` fixture、授权集合被误当作完成清单、已恢复失败永久污染终态，以及中断时只保留最后一次工具结果。对应修复复用通用否定策略、跨确认分轮执行、按实际调用维护收据并累计真实产物。

## 测试

- `cargo test --manifest-path src-tauri/Cargo.toml agentloop::native::tests -- --nocapture`
- `cargo test --manifest-path src-tauri/Cargo.toml agentloop::native_policy::tests -- --nocapture`
- `cargo test --manifest-path src-tauri/Cargo.toml --test agent_contract_assets -- --nocapture`
- `node scripts/test-agent-contracts.mjs`
- 提交前完成 `npm run agent:check`、`npm run lint`、`npm run build`、`npm run harness:test`、`npm run harness:check`、Rust fmt/check/test 与 Python unittest。

## 同步文档

- `AGENTS.md`
- `README.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/harness.md`
- `docs/codebase/ARCHITECTURE.md`
- `docs/codebase/STRUCTURE.md`
