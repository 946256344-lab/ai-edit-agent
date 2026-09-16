# 默认直推 master，提交不再走分支和完整检查

## 结果

单 Agent 默认在 `master` 提交并 `git push origin master`。不再为每个改动建分支、开 PR，也不在每次提交前跑 `harness:test`、`npm run build`、`cargo test`。pre-commit 只拒绝 detached HEAD，并检查文档同步。多个 Agent 并行时才用独立分支和 worktree。

## 范围

- `CONTRIBUTING.md`：默认 master 提交推送；检查按改动补跑
- `.harness/branch-policy.json`、`scripts/check-branch-policy.mjs`、`scripts/test-branch-policy.mjs`、`.githooks/pre-commit`
- `docs/architecture.md`、`docs/decisions.md`、`docs/harness.md`、`docs/roadmap.md`、`README.md`
- `docs/codebase/CONVENTIONS.md`、`docs/codebase/TESTING.md`、`docs/codebase/CONCERNS.md`、`docs/codebase/STRUCTURE.md`

## 禁止变化

- 不改公开 Tauri 命令或产品行为
- 不删除文档同步检查
- 不启用远端分支保护

## 验证

- `npm run branch:test`：通过
- `npm run branch:check`：通过，允许当前分支提交
- `npm run harness:check`：架构预算通过；Agent 契约仍被既有 `outbound_http.rs` 网络白名单阻断，该文件本轮未修改
