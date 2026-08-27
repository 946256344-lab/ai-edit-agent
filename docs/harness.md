# 文档与检查说明

`npm run harness:check` 会串联仓库当前的三项检查：架构预算、Agent 上下文和文档同步。它们的目的只是防止明显的结构漂移，不是替代代码审查或测试。

## 什么时候会检查

- 改了公开 Tauri 命令、前端 bridge 或本地存储入口时，确认 `docs/api.md` 同步更新。
- 改了 `AGENTS.md`、`CONTRIBUTING.md`、`TASKS.md` 或 `docs/codebase/` 时，确认相关说明仍然一致。
- 改了 `.harness/` 里的检查配置时，确认脚本还能读懂最新结构。

## 当前检查命令

```powershell
npm run harness:install
npm run architecture:check
npm run agent:check
npm run harness:check
```

## 说明

- 架构预算只用于防止明显回流，不追求把文件大小当成质量指标。
- Agent 上下文文件只描述当前约定，不承担完整项目历史。
- 文档同步主要面向公开接口和明显边界变化；普通小改动以代码和测试为主。

## 证据

- `.harness/architecture-budgets.json`
- `.harness/agent-context.json`
- `.harness/doc-sync-policy.json`
