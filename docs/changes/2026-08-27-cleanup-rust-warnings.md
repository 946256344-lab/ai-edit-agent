# Rust 后端 dead-code 警告清理

## 结果

- 删除未使用的 import、变量、场景检测遗留代码，以及已被三阶段 storyboard 替代的旧 `request_storyboard` 入口。
- 对前端/审计/对抗验证等预留契约保留最小范围 `#[allow(dead_code)]` 并注明理由。
- 不改 Tauri 命令名、SQLite schema、Agent 工具白名单、Provider 协议或 storyboard/timeline/preview 行为。

## 文档同步

- `AGENTS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/harness.md`
- `README.md`
- `TASKS.md`
