# Phase 1 + Phase 2: 让错误可见并给 Agent 松绑

## Phase 1: 让错误可见 - 移除 Provider 自动重试与无效调用守卫

- 移除 `NATIVE_MODEL_MAX_ATTEMPTS=3` 重试循环、`wait_for_native_model_retry()`、`native_model_attempt_timeout()` 与 `NATIVE_MODEL_RETRY_DELAY`。每个模型步骤只做一次诚实尝试，网络抖动与真 bug 不再被悄悄吞掉。
- Provider 失败直接透传错误；`classify_model_request_failure()` 保留稳定错误码（`provider_http_429` 等）用于排障提示，但不再携带 `retryable` 自动重试决策。错误码仍写入 Agent 诊断，前端直接看到真因。
- 移除 `InvalidCallGuard` / `native_call_signature` / `canonical_json_value` 的正则化去重与 3 次熔断。重复无效参数不再被守卫拦截，由模型根据 `invalid_arguments` 返回自行调整。
- 单个模型步骤获得完整步骤预算（不再三等分给 3 次尝试）。

## Phase 2: 给 Agent 松绑 - 简化工具策略并移除动态加载

- `RequestToolPolicy` 只剩 `read_only` 一档：`read_only = 请求包含"只"或小写 "only"`。删除 20+ 负向关键词黑名单、敏感能力授权与期望写工具清单。
- `native_tool_call_allowed()` 简化为 `!read_only || is_observation`；模型默认获得全部可逆本地工具，目录可见性不再区分加载状态。
- 删除 `load_tools` 动态加载与 5 个工具上限；每轮直接向 Provider 注册全部 24 个工具的完整 strict schema。
- 删除 `expected_native_write_tools` 驱动的 unverified 终态收据；模型自然语言收尾直接按真实失败/挂起工具判定终态。
- 同步删除 `LoopState.loaded_tools`、`dynamic_tool_loaded` / `tool_not_loaded` / `load_request_uses_available_names`，前端 `agent-tools.ts` 与 `AgentRunCard` 移除 `load_tools` 镜像。

## 契约与测试

- 契约 fixture `agent_tool_contracts.v1.json` 从 25 个工具收缩为 24 个，白名单测试同步更新。
- `native.rs` 重试/守卫单测替换为契约测试：Provider 在工具输出后失败时立即透传、不重试、不重跑工具。
- 5 个旧语义测试改为新行为断言（无工具声称完成 → Completed；确认挂起时全量目录仍可见、执行才被拦）。
- 不改变 SQLite schema、公开 Tauri 命令或领域产物。`cargo test` 全部通过、零告警，前端 `tsc -b` 通过。

## 文档同步

- `docs/api.md`
