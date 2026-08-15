# 代码库地图与 IDE 导航

## 背景

前端领域边界已经收敛，但 Rust 后端仍有多个大型文件。直接按行数拆分会跨越 task receipt、Agent 终态事务、媒体 worker 恢复和产物版本边界；同时，现有长期文档偏产品契约，不适合作为源码学习入口。

## 实现

- 按真实源码和终端扫描生成 `docs/codebase/` 七份代码库文档，覆盖栈、结构、架构、约定、集成、测试和风险。
- 在 Rust crate 模块注册处、SQLite 边界和 native process 入口增加职责型 Rustdoc/注释；在前端四个领域 controller、renderer 入口和 Python Jianying adapter 增加导航型注释。
- 将未参与运行时的 `src/lib/agent-tools.ts` 从历史工具名更新为当前 Rust 9 个观察技能、12 个编辑/交付技能和 canonical control actions，并明确 Rust/fixture 是执行事实。
- 记录 `agentloop.rs` 与 `assets.rs` 的渐进拆分边界和顺序；本次不移动实现、不改变命令或副作用。

## 不变边界

- 不改变 Tauri 命令名、参数或响应。
- 不改变 SQLite schema、迁移、查询或事务。
- 不改变 Agent 运行时白名单、Provider、媒体分析、storyboard、timeline、preview 或 Jianying 行为。
- 不删除、迁移或重新分析用户数据。

## 同步文档

- `README.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/roadmap.md`
- `docs/codebase/*.md`

## 验证

- `docs/codebase/` exact-file、inquiry/evidence 与 `[ASK USER]` 清单检查通过；目录中恰好七份规定文档。
- `npm run lint`、`npm run build`、`cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo check --manifest-path src-tauri/Cargo.toml` 通过。
- `cargo test --manifest-path src-tauri/Cargo.toml` 通过：128 个单元测试、2 个跨模块契约测试；既存 `AgentLoopControl::PartiallyDone` dead-code warning 未冒充绿色修复。
- Python Jianying adapter 14 个测试、`npm run harness:test`、`npm run harness:check` 与 `git diff --check` 通过。
- 独立 Agent 三轮审查关闭 control canonical/alias、`get_edit_status`、Rust trust boundary、Task Resolver 澄清与 receipt 时序、SQLite 命令范围和术语精度问题；最终无 blocker。
