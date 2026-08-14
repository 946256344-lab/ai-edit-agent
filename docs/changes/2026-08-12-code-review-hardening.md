# 代码审查加固

## 范围

修复四项代码审查问题，不改变公开 Tauri 参数或 SQLite schema：

- 将文本轨 Jianying 兼容性判断改为 Rust 1.77.2 可用的 `Option::map_or`，保持仓库声明的最低 Rust 版本。
- Task Resolver 继续限制最近 12 个任务快照，但无条件额外包含仍属于当前项目的显式活动任务，避免旧任务被候选上限截断。
- Python Jianying 适配器先验证唯一 draft 名只能解析为草稿根目录内的单层目录；目录创建后的轨道构建、保存或注册失败时，只回滚本次未成功交付的新目录，既有 draft 不会被删除或覆盖。
- Provider 安全文档 harness 增加 `custom_api.rs` 与 `music_provider.rs`，覆盖全部当前 Windows Credential Manager 凭据模块。

## 回归覆盖

- Rust：活动任务位于最近 12 条之外时仍进入候选。
- Python：拒绝越出草稿根目录的名称；`create_draft` 自身部分失败或后续轨道创建失败时，新 draft 目录均被移除。
- Harness：自定义 API 与 Jamendo 凭据模块变更必须要求 `AGENTS.md` 和 `docs/decisions.md`。

验证通过：97 个 Rust 单元测试、2 个 Agent 契约测试、14 个 Python 适配器测试、`npm run lint`、`npm run build`、`cargo fmt --check`、Clippy 的 `incompatible_msrv` 检查、`npm run harness:test`、`npm run harness:check` 与 `git diff --check`。

## 同步文档

- `AGENTS.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/harness.md`
