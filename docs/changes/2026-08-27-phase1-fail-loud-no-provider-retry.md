# Phase 1: 让错误可见 - 移除 Provider 自动重试与无效调用守卫

## 结果

- 移除 `NATIVE_MODEL_MAX_ATTEMPTS=3` 重试循环、`wait_for_native_model_retry()`、`native_model_attempt_timeout()` 与 `NATIVE_MODEL_RETRY_DELAY`。每个模型步骤只做一次诚实尝试，网络抖动与真 bug 不再被悄悄吞掉。
- Provider 失败直接透传错误；`classify_model_request_failure()` 保留稳定错误码（`provider_http_429` 等）用于排障提示，但不再携带 `retryable` 自动重试决策。错误码仍写入 Agent 诊断，前端直接看到真因。
- 移除 `InvalidCallGuard` / `native_call_signature` / `canonical_json_value` 的正则化去重与 3 次熔断。重复无效参数不再被守卫拦截，由模型根据 `invalid_arguments` 返回自行调整。
- 单个模型步骤获得完整步骤预算（不再三等分给 3 次尝试）。

## 契约与测试

- `native.rs` 6 个重试/守卫单测替换为 1 个契约测试：Provider 在工具输出后失败时立即透传、不重试、不重跑工具。
- `provider.rs` 分类测试改为只断言稳定错误码，不再断言 retryable。
- 不改变 SQLite schema、工具白名单、公开 Tauri 命令或领域产物。

## 文档同步

- `docs/api.md`
