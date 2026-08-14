# Agent 审计运行时

## 触发范围

- `src-tauri/src/store.rs`、`src-tauri/src/lib.rs`：SQLite 持久化、Tauri 命令和启动恢复。
- `src/lib/local-store.ts`、`src/lib/agent-tools.ts`：桌面工具查询契约。
- `src/App.tsx`、`src/components/AgentAuditPanel.tsx`、`src/App.css`：当前剪辑会话的审计界面。

## 改动

- 将 schema 升级至 version 4，为 Agent 调用和操作日志加入剪辑任务、会话和调用关联。
- `execute_agent_edit` 在模型调用前持久化调用状态，并仅保存请求长度、版本标识、脱敏结果和固定安全错误；副作用日志关联对应调用。
- 工具目标契约继续使用 `analyze_assets`，与现有内部 `analyze_asset` 分析任务区分，避免将内部队列名暴露为通用工具 API。
- 工具失败改为将安全结构化结果持久化并回传模型生成后续回复，不再由 UI 直接展示技术校验错误；Provider 或初始模型决策不可用时使用持久化安全结果和固定降级回复。
- 若模型复述安全失败代码，后端拒绝该回复并使用固定自然语言说明。
- 暂停或中断的通用 Agent 调用在启动时转为 `needs_review`，系统不会自动重放未知副作用。
- 新增作用域化的 Agent 调用、操作日志和时间线版本查询命令，并在 UI 显示当前会话的审计摘要。

## 同步文档

- `AGENTS.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/changes/2026-08-05-agent-audit-runtime.md`

## 验证

- `npm run lint` 通过。
- `npm run build` 通过。
- `cargo test` 通过 12 项；3 项依赖认证实验性 Provider 的集成测试按设计跳过。

## 决策

- 新增 ADR-025：中断的通用 Agent 调用必须待审阅，不能自动重放未知副作用。
