# Agent 可靠运行时第一阶段

## 变更

- SQLite schema 升至 v5，新增 payload-free `agent_run_steps` 和三重作用域查询。
- 循环技能及显式直通技能记录开始、完成或安全失败；中断步骤封闭为 `interrupted_requires_review`，不自动重放。
- 每轮模型决策前重建紧凑 `AgentStateSnapshot`，加入确定性前置条件提示。
- Agent 终态增加 `partially_completed` 与 `needs_clarification`。
- 增加版本化工具契约、Agent 回归用例和白名单一致性测试。
- 显式无效 timeline ID 不再回退到唯一候选时间线。

## 文档同步

- `AGENTS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/roadmap.md`
- `TASKS.md`

## 尚未完成

- 持久化队列、暂停/恢复和人工审阅后的续跑入口。
- timeline/storyboard 版本撤销、分支与比较操作。
- 最终导出、覆盖和删除的一次性 approval token 两阶段协议。
- 真实 Provider 与真实媒体的桌面端回归运行。
