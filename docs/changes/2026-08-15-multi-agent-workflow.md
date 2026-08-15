# 多 Agent 统一协作流程

## 结果

- 新增 `CONTRIBUTING.md` 作为 Cursor、Codex、Claude Code、OpenCode 共用的分支、worktree、验证、提交和 PR 唯一事实源，不分配各工具职责。
- 新增 Claude Code、Cursor 和 OpenCode 薄入口及 PR 模板；Agent 契约门验证入口持续引用权威文件，并以只收紧 ratchet 阻止删除。
- 新增分支策略配置、检查器和负向测试；pre-commit 现在拒绝受保护分支、无效前缀、detached HEAD 和未包含本地 `origin/master` 的任务分支。
- 未修改产品运行时、公开 Tauri 命令、SQLite、媒体、Provider、Agent 工具或用户数据；未合并或推送 master。

## 同步文档

- `AGENTS.md`
- `CONTRIBUTING.md`
- `README.md`
- `docs/architecture.md`
- `docs/decisions.md`
- `docs/harness.md`
- `docs/roadmap.md`
- `docs/codebase/STRUCTURE.md`
- `docs/codebase/CONVENTIONS.md`
- `docs/codebase/TESTING.md`
- `docs/codebase/CONCERNS.md`
- `TASKS.md`
