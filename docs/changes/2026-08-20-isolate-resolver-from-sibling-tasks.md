# 2026-08-20: Task Resolver 不再看见兄弟剪辑任务

## 问题

Native 历史已按 `conversation_id` + `editing_task_id` 隔离，但 Task Resolver 仍把同一项目内最近 12 个任务的 title、brief 和最多 240 字的 `active_subgoal`（最近一次已归属请求摘要）交给路由模型，用于 continue/switch/create。其他剪辑会话的名称和最近请求因此进入当前会话的归属判断。

## 修复方案

- `load_task_candidates` 只加载当前激活且属于该项目的剪辑任务；没有激活任务时候选为空，走既有 `create_new` 快路径。
- 路由 prompt 只包含当前任务快照，不再提供 `switch_existing`，也不要求模型按名称切换其他任务。
- 低置信度澄清只问“继续当前任务还是创建新任务”，不列举其他任务名称。
- 模型若仍返回 `switch_existing` 且目标不是当前候选，继续按 out-of-scope 失败封闭。
- 侧栏激活另一任务后，后续消息以该任务为当前任务走 `continue_current`；不改变 SQLite schema、公开命令名或 NativeToolLoop。

## 变更范围

**Rust 内部行为**：
- `taskrouter.rs::load_task_candidates`
- `taskrouter.rs::build_task_route_prompt`
- `taskrouter.rs::ambiguous_route_result`

**不变更**：
- Tauri 命令签名
- 前端 TypeScript 接口（`switch_existing` 仍可出现在历史 receipt）
- SQLite schema
- Native 消息历史 JOIN

## 同步文档

触发规则 `desktop-contract` 要求同步：`docs/architecture.md`、`docs/api.md`、`TASKS.md`。

- [x] `docs/architecture.md`：会话隔离与 Task Resolver 改为只看见当前激活任务。
- [x] `docs/api.md`：`resolve_conversation_task` 候选从最近 12 个任务改为仅当前激活任务。
- [x] `TASKS.md`：登记当前任务窗口。
- [x] `docs/decisions.md`：ADR-046 补充不再按名称切换兄弟任务。
- [x] `README.md`：无需变更。
- [x] `docs/harness.md`：无需变更。

## 触发规则

- `desktop-contract`：`src-tauri/src/taskrouter.rs` 改变任务归属输入。

## 验证

- [x] `resolver_candidates_exclude_sibling_task_identity`：兄弟任务 title/`active_subgoal` 不得进入 prompt；`switch_existing` 到兄弟任务失败封闭
- [x] Rust 库测试 178 + 2 个契约测试
- [x] `npm run harness:test` / `npm run harness:check`（desktop-contract）
- [x] 本收尾无前端改动，未跑 `npm run lint` / `npm run build`
