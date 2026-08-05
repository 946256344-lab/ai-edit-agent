# 2026-08-05 文档同步 harness

## 触发范围

- `package.json`：新增 harness 命令。
- `scripts/`、`.harness/`、`.githooks/`：新增可执行的文档同步检查、单元测试与提交前入口。

## 改动

建立基于 Git 变更集的文档同步硬检查。高影响的桌面命令、持久化、OAuth、安全和运行时配置改动必须同时更新映射的长期文档，并提供架构变更记录。检查覆盖新增、修改、重命名和删除；提交前检查只读取暂存内容。Agent 在提交前执行独立上下文审查和修复 loop。

## 同步文档

- `AGENTS.md`
- `README.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/harness.md`

## 验证

- `npm run harness:test`
- `npm run harness:check`
- `npm run lint`
- `npm run build`

## 决策

新增 ADR-023。
