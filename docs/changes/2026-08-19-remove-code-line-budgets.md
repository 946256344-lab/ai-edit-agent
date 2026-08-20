# 取消代码文件行数预算

## 目标

移除架构预算对代码文件和目录行数的硬限制，同时保留能够约束职责膨胀、代码密度和跨层回流的其它机器检查。

## 变更

- 从 `.harness/architecture-budgets.json` 删除所有路径和目录 `maxLines` 字段。
- 从 `scripts/check-architecture-budgets.mjs` 删除行数度量、检查和 ratchet；旧基线中的 `maxLines` 删除不会被视为预算放宽。
- 更新 `scripts/test-architecture-budgets.mjs`，固定验证超长行数不再失败、字符/单行/hooks/props/禁止边界仍受保护，并覆盖旧 `maxLines` 基线兼容。
- 同步 `docs/harness.md`、`docs/architecture.md`、`docs/decisions.md`、`docs/codebase/CONVENTIONS.md` 与 `TASKS.md`。

## 保留边界

字符总量、最长单行、hooks、props、禁止路径、禁止文本、跨层边界、文档同步和 Agent 契约检查不变。此变更不修改 Agent Runtime、Provider、Router、LoopGoal、SQLite、权限或媒体处理。

## 验证

已通过：`npm run architecture:test`、`npm run agent:check`、`npm run harness:test`、`npm run harness:check`、`npm run architecture:check`、`npm run lint`、`npm run build`、`npm run branch:check` 和 `git diff --check`。
