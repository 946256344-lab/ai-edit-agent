# 自定义 OpenAI 兼容模型 API 入口

日期：2026-08-07

## 变更

为模型 Provider 增加第二个可控入口：用户可配置任何 OpenAI 兼容的自定义 API（Base URL + Model 名 + API Key），凭据仅保存于 Windows Credential Manager。配置后 `ModelAccess::resolve()` 优先生效自定义 API，否则回退到实验性 OpenAI OAuth。模型请求层统一通过 `ModelAccess`（OAuth Responses 或 Chat Completions）分发，应用逻辑不直接绑定任一端点格式。

## 触发的规则与同步文档

- 新增 `custom_api.rs` 模块与 `get_custom_api_status` / `save_custom_api` / `clear_custom_api` 命令；`provider.rs` 增加 chat/completions 适配与 `ModelAccess` 决策分派；`agent.rs`、`storyboard.rs`、`assets.rs` 改用 `ModelAccess::resolve()`。
- 同步 `docs/api.md`（新增命令与 Provider 决策规则）、`docs/architecture.md`（系统边界与数据流）、`docs/decisions.md`（ADR）、`AGENTS.md`、`TASKS.md`。

## 验证

- `cargo build --lib` 通过；`cargo test --lib` 15 通过、3 项依赖认证 Provider 的集成测试跳过。
- `npm run lint`、`npm run build`、`npm run harness:check` 通过。

## 未决项

- 自定义 API 的端到端真实托管模型响应仍未人工验证；视觉分析通过自定义 API 时的图片 URL 与超时行为待桌面验证。