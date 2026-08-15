# Agent policy 拆分与中文源码导航

## 目标

在不改变产品行为、公开命令、持久化和用户数据的前提下，完成后端热点的第一条物理边界，并让人工或 AI Agent 能在 IDE 中先理解每个源码模块的职责。

## 实现

- 新建 `src-tauri/src/agentloop/policy.rs`，迁移观察/副作用工具白名单、`RequestToolPolicy`、目标快路径、模型目标解析、真实产物完成门和固定诚实降级文案。
- policy 只依赖 `AgentEditResult` 数据类型，不持有 SQLite、路径、Tauri、Provider 或进程能力；父 `agentloop.rs` 继续拥有 Router、状态快照、prompt、有界循环和 `apply_skill`。
- Agent 契约测试、版本化 fixture 和检查器改从 policy 读取白名单；文档同步规则新增递归 Rust 子模块匹配。
- `.harness/agent-context.json` 新增源码导航清单；`agent:check`/pre-commit 检查文件头 16 行内的中文注释，并以 ratchet 阻止未来移除或放宽该门。
- `agentloop.rs` 架构预算由 4264 行收紧到 3599 行，并为 674 行 policy 建立独立预算，防止职责重新合并。
- 全部手写 Rust、TypeScript/React、Node、Python、HTML、Shell 与 CSS 源码补中文职责导航；route receipt、Agent 终态事务、完成门、Provider 回退、旧素材目录恢复和 timeline 版本等关键机制补就地中文解释。

## 保持不变

- Tauri command 名称、参数与 camelCase 响应不变。
- SQLite schema、事务语义、Agent 最大步数、工具名称和 fixture 工具集合不变。
- Provider 选择、素材分析、storyboard、timeline、preview、Jianying 和用户本地文件行为不变。
- 没有删除、重分析或重新生成任何用户素材与产物。

## 验证

- `cargo fmt --check`、`cargo check`、128 个 Rust 单元测试与 2 个契约测试通过；仅保留既有 `PartiallyDone` dead-code warning。
- `npm run lint` 与 `npm run build` 通过。
- Python Jianying adapter 14 个测试通过。
- `agent:test/check`、`harness:test/check/staged`、pre-commit、diff 检查通过。
- 独立审查发现并关闭 Shell exact-file 漏检、`search_music` 注释失真与重复断言；最终无 blocker/high/medium。

## 同步文档

- `AGENTS.md`
- `README.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/roadmap.md`
- `docs/harness.md`
- `docs/codebase/STRUCTURE.md`
- `docs/codebase/ARCHITECTURE.md`
- `docs/codebase/CONVENTIONS.md`
- `docs/codebase/TESTING.md`
- `docs/codebase/CONCERNS.md`

## 后续

下一条纯边界是 `assets/library.rs`：先迁移安全目录投影和只读查询，再继续 `agentloop/router/state`。worker、事务和技能派发仍最后处理。
