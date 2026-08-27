# Rust 后端说明

本目录为 `src-tauri/src/`，负责命令、SQLite、Agent、媒体和交付。

- React 只表达意图，Rust 校验作用域并决定真实副作用和完成状态。
- Tauri 命令在 `src-tauri/src/lib.rs` 注册，变更时同步 `src/lib/local-store.ts` 和 `docs/api.md`。
- `process`、`provider`、凭据相关代码各有归属，不要随意分散。
- SQLite 事务和失败恢复以代码为准，保持可审计和可重现。
- 错误返回真实原因但不泄露敏感信息；不让假成功掩盖问题。
- `agentloop.rs` 与 `assets.rs` 是持续演进的热点，后续改动尽量按已有模块边界拆分，不要把新职责堆回单文件。
