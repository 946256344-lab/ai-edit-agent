# 2026-08-14 绿色构建恢复

## 触发范围

- `src-tauri/src/agent.rs` 与 `src-tauri/src/storyboard.rs` 触发 `desktop-contract` 文档同步规则；`agent.rs` 同时触发 `provider-security` 规则。
- `src/App.tsx` 与 `src/components/ConversationWorkspace.tsx` 包含前端 TypeScript、失效实现和 Hook 依赖清理。

## 改动

- 移除 `App.tsx` 中已经由领域组件取代、且不再被引用的素材列表旧实现与状态。
- 补齐 `AssetRelinkPreview` 类型导入，移除未使用的 `timelineState` 组件契约，并将交付状态同步逻辑收回对应 effect，消除 lint 警告。
- 对 Rust 文件执行标准 `rustfmt`；没有改变运行逻辑。
- 没有改变 Tauri API、Agent 工具契约、持久化 schema、媒体分析、timeline、preview 或 Jianying 行为。

## 同步文档

- `AGENTS.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`

## 验证

- `npm run lint`：通过，零 lint 警告。
- `npm run build`：通过，TypeScript 与 Vite 生产构建成功。
- `cargo fmt --check`：通过。
- `cargo test`：通过，112 个单元测试和 2 个契约测试无失败；保留一个不阻塞构建的既有 `PartiallyDone` 未构造警告。
- `npm run harness:check`：通过，`desktop-contract` 与 `provider-security` 文档规则均满足。
- `git diff --check`：通过。
- 真实 Tauri 桌面核心闭环不在本次绿色构建恢复范围内，已重新列为 P0 待验收。

## 决策

无新增架构决策。
