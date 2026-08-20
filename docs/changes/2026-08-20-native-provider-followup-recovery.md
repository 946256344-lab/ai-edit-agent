# Native 工具后续模型回复恢复

## 目标

修复 NativeToolLoop 已成功执行工具并追加 `function_call_output` 后，下一次 Provider 总结请求遇到瞬时传输失败时直接显示固定恢复文案的问题。修复必须保留真实工具收据、超时/取消边界和不重复副作用原则，并让后续诊断能区分安全失败类别。

## 根因与历史证据

桌面历史中的三次失败都满足同一证据链：第一轮 Provider 响应已产生 `function_call`，对应观察工具成功并记录耗时；之后没有第二条 `native_response_bytes`。这证明中断发生在工具成功、原生 `function_call`/`function_call_output` 已准备好之后，以及第二次 Provider 响应体到达之前。它不是工具失败、UI 覆盖模型文本或响应解析失败。

旧诊断只保存 Provider 选择与成功响应长度，没有保存请求失败类别，因此历史数据不能诚实区分 HTTP 429、5xx、超时或网络中断。新诊断只投影安全码和尝试次数，不能把 URL、模型名、凭据、响应正文或传输详情写入 SQLite。

## 循环与修复

```text
SQLite user/assistant 历史 + 当前 user
  -> Provider 返回 message 或 function_call
  -> Rust 对 function_call 做请求策略、参数、作用域和权限复核
  -> apply_skill 执行一次，RunReceipt 记录真实结果
  -> 追加完整 function_call + 结构化 function_call_output
  -> Provider 总结步骤
       -> 瞬时故障：同一 payload 在剩余预算内最多三次尝试
       -> 永久错误：立即失败
  -> 自然语言回复
  -> RunReceipt/持久化事实决定终态并保存 assistant 消息
```

- `provider.rs` 新增 `ModelRequestFailureClass` 和 `classify_model_request_failure`，将 Provider 传输错误投影为 `provider_http_<status>`、`provider_timeout`、`provider_network`、`provider_empty_response` 或 `provider_unknown`。HTTP 状态只从错误后缀 `:HTTP {status}` 解析，避免 Base URL 中的 `HTTP`/`HTTPS` 被当成状态码。
- HTTP 408/425/429/500/502/503/504、超时、网络中断与空响应可重试；永久 4xx 与未知错误不重试。
- `agentloop/native.rs` 为每个逻辑模型步骤增加最多三次的 Provider 请求尝试，延迟基数为 350ms。所有尝试共享当前 120 秒单步剩余时间与 300 秒总运行预算；每一次 HTTP 只使用剩余预算除以剩余次数的份额，避免一次挂起耗尽 120 秒后无法重试。每次尝试前和退避期间每 50ms 检查任务取消，取消后不再发下一请求。
- 重试闭包只调用 `post_model_payload`，位于 function_call 解析和工具执行之外；工具成功后 Provider 重试不会再次调用 `execute_native_tool` 或 `apply_skill`。
- Agent 诊断复用既有 SQLite `pipeline_error` 类型，安全内容以 `provider_retry`、`provider_recovery` 或 `provider_failure` 开头，并只含安全码和尝试次数；不扩大持久化 schema。
- Provider 仍持续不可用时，NativeToolLoop 使用既有诚实恢复路径；RunReceipt 已确认的产物不会被模型文本覆盖或虚假标记。

## 测试

- `provider::tests::model_request_failure_classification_is_safe_and_retryable_only_when_transient`：429 可重试、400 不可重试、超时可重试，安全码不含 URL 或模型名；大写 `HTTP://192.168.0.1:8080` 的超时仍分类为 `provider_timeout`。
- `agentloop::native::tests::transient_provider_failure_after_tool_output_retries_without_reexecuting_tool`：`list_assets` 成功后第二个模型步骤先返回 429、再恢复自然语言；Provider 共三次请求，工具只执行一次，诊断顺序为 retry → recovery。
- `agentloop::native::tests::empty_provider_response_after_tool_output_retries_without_reexecuting_tool`：第二个模型步骤返回空正文后同样重试恢复，工具仍只执行一次。
- `agentloop::native::tests::permanent_provider_failure_after_tool_output_is_not_retried_or_reexecuted`：`list_assets` 成功后第二个模型步骤返回 400；该步骤只尝试一次，工具仍只执行一次，诊断只保存 `provider_http_400`。
- `agentloop::native::tests::cancellation_during_retry_backoff_stops_before_the_next_provider_attempt`：第一次 429 后在退避阶段取消，后续 Provider 请求不再发送。
- `agentloop::native::tests::retryable_network_failure_splits_step_budget_so_later_attempts_can_run`：网络中断时三次 HTTP 各自超时都小于 120 秒单步预算，失败诊断为 `provider_network` 且 `attempts=3`。
- `agentloop::native::tests::native_model_attempt_timeout_keeps_two_thirds_of_a_fresh_step_for_later_tries`：新鲜 120 秒步骤第一次只取 40 秒，后两次仍能各得 40 秒。
- Provider 分类回归覆盖 HTTP 200 空正文产生的 `Provider response was empty.`，确保投影为可重试的 `provider_empty_response`。
- `audit::tests::provider_retry_diagnostics_persist_only_safe_codes_and_attempt_counts`：retry/recovery/failure 阶段复用现有 `pipeline_error` schema，并能实际持久化安全码与次数。
- 固定 JSON/闭包 fixture，不调用真实 API，不包含真实凭据、本机路径或原始敏感响应。

完整 Provider INPUT/OUTPUT 检查器已拆到 `chore/native-provider-trace`，不在本修复范围内。

## 验证状态

- [x] 新增范围测试通过，含 429、空响应、永久 400、退避中取消和大写 `HTTP` Base URL 超时分类。
- [x] 完整 Rust、TypeScript、Python、agent 与 harness 完成门：171 个 Rust 单元测试、2 个 Rust 契约测试、14 个 Python 测试、agent/branch/harness、fmt/check 与 diff 检查通过。本跟进无前端改动，未重跑 lint/build。
- [x] 独立只读审查：关闭“退避期间未检查取消”“英文空正文未命中分类”“Base URL 中 HTTP 被当成状态码”“空响应缺少循环 fixture”。超时拆分跟进再审无阻塞：每次 HTTP 只用剩余预算份额，120/300 未扩大，不重复工具，诊断不泄密，永久 4xx 不重试，取消边界保持。
- [x] 桌面复测：同一问句后 Agent task 为 `completed`，对话返回自然语言素材计数，未出现固定恢复文案。本轮无 `provider_retry`/`provider_failure` 诊断（挂起未复现）。此前失败轮为 `provider_network` 且 `attempts=1`、总耗时约 178 秒；按剩余次数拆分超时后，该路径不再把整段 120 秒交给第一次 HTTP。

## 已知未改路径

步骤预算耗尽时 `request_native_model_with_retry` 仍返回 `native_tool_loop_deadline_exceeded`，UI 会显示总超时文案。瞬时 429 的快失败走原始 Provider 错误，不经过该映射。完整 Provider INPUT/OUTPUT 检查器已拆到 `chore/native-provider-trace`。

## 同步文档

- `AGENTS.md`
- `README.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/harness.md`
- `docs/codebase/INTEGRATIONS.md`
