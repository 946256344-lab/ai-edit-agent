# Rust 后端模块拆分与 Agent 决策层解耦

## 触发范围

- `src-tauri/src/store.rs`：被删除，拆分为以下模块。
- `src-tauri/src/{db,models,process,provider,audit,projects,assets,storyboard,timeline,preview,jianying,agent}.rs`：新模块。
- `src-tauri/src/lib.rs`：模块声明与命令注册映射更新。
- `.harness/doc-sync-policy.json`：`desktop-contract` 的源码路径从 `src-tauri/src/store.rs` 更新为 `src-tauri/src/*.rs`，`provider-security` 增加 `provider.rs`、`agent.rs`。

## 改动

- 将 4190 行的 `store.rs` 单体模块按职责拆分为 `db`（迁移与连接）、`models`（领域类型）、`process`（无窗口外部命令）、`provider`（实验性模型请求）、`audit`（Agent 调用与操作日志）、`projects`、`assets`、`storyboard`、`timeline`、`preview`、`jianying`、`agent`。
- 全部 Tauri 命令名、入参与出参保持不变；`src/lib/local-store.ts` 无任何改动。
- Agent 控制器集中在 `agent.rs`；`execute_agent_edit` 保持单一入口，并通过 `ToolDecisionProvider` trait 与模型决策层解耦。决策 provider 只负责把请求、brief、storyboard 状态、时间线候选与媒体证据交给模型并解析工具决策；副作用执行、作用域校验与审计保留在控制器内。Provider 可替换而不改控制器契约。
- `request_agent_edit_decision` 保留为 `#[cfg(test)]` 测试辅助自由函数，生产路径经 trait 对象调用。
- schema version 保持 4 不升级；迁移 SQL 按原 `store.rs` 重建，保持幂等方式（pragma 列检测、legacy 回填、operation_logs 回填）一致。
- 为 `StoryboardSource`、`SceneSegment`、`OcrEvidence`、`VisualEvidence` 补充 `Clone` derive 以支持测试构造。
- 修复 `preview.rs`、`storyboard.rs` 的缺失导入，并为 `db.rs`、`preview.rs`、`timeline.rs` 补充 `tauri::Manager` 导入。

## 同步文档

- `TASKS.md`
- `docs/architecture.md`
- `docs/decisions.md`
- `docs/changes/2026-08-06-rust-module-split.md`

`docs/api.md` 与 `AGENTS.md` 在本拆分中无需内容修改（命令契约与产品边界不变），由同一工作区内既有的 2026-08-05 变更记录（`agent-audit-runtime.md`、`visual-analysis-timeout-and-oauth-logout.md`）同步覆盖，从而满足 `desktop-contract` 与 `provider-security` 规则的变更记录要求。

## 验证

- `cargo check` 无错误无警告。
- `cargo test` 通过 13 项；3 项依赖认证实验性 Provider 的集成测试按设计跳过，与拆分前一致。
- `npm run lint` 通过（0 警告 0 错误）。
- `npm run build` 通过。
- `npm run harness:check` 通过（desktop-contract、provider-security）。

## 决策

- 新增 ADR-027：Rust 后端按职责拆分，Agent 决策层经 `ToolDecisionProvider` trait 与副作用执行层解耦，`execute_agent_edit` 保持唯一入口与既有命令契约。
