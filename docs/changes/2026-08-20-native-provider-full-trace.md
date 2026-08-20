# Native Provider 完整输入/输出转储

## 目标

让本机调试能直接阅读 NativeToolLoop 每次真实 HTTP 的完整 INPUT/OUTPUT。不在前端展示，不新增 Tauri 命令，不把原文写入 SQLite、浏览器存储或普通产品日志。

## 循环与边界

```text
debug 构建 + NATIVE_PROVIDER_FULL_TRACE=1
  -> NativeToolLoop 每次真实 HTTP 尝试
       -> request 行：实际发送的完整 JSON
       -> response 行：服务器正文 + HTTP 状态；无响应则不伪造 OUTPUT
  -> 追加到 src-tauri/target/native-provider-full-trace.jsonl
release 或未设置开关：不写文件
```

- 新模块 `agentloop/trace.rs` 拥有开关、JSONL 追加和 `recordId`。进程首次开启时截断旧文件。
- `provider.rs` 的 wire observer 只把响应正文交给转储，并精确遮蔽当前 Provider 凭据与 Base URL；请求头从不进入文件。
- Native 重试的每一次 HTTP 都有独立 INPUT/OUTPUT；不重放工具，不扩大 120/300 秒预算。
- `npm run tauri:dev` 设置该开关；不在前端展示原文。

## 测试

- `agentloop::trace::tests`：必须同时满足 debug 与显式开关；自定义 Chat 适配后的完整输入保留 tool call 上下文；序列化不含 API Key、Base URL 或 Authorization；JSONL 追加两行且路径留在 `target/`。
- `provider::tests::wire_observer_receives_complete_http_error_body_without_request_credentials`：本机 HTTP 400 fixture 证明错误正文可进入内存 observer，凭据只出现在真实请求头。

## 验证状态

- [x] 转储测试通过：开关、Chat 适配正文、JSONL 追加、路径留在 `target/`、HTTP 400 observer 不带入凭据。
- [x] 桌面：`npm run tauri:dev` 后真实 Agent 请求写入 JSONL，可按 step/attempt 阅读 INPUT 与有正文时的 OUTPUT。

## 已知未改路径

storyboard / 粗视觉等非 NativeToolLoop 模型请求不进入该转储。

## 同步文档

- `AGENTS.md`
- `README.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/harness.md`
- `docs/codebase/INTEGRATIONS.md`
