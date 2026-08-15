# 编码 Agent 上下文与契约硬门

## 背景

仓库已有大量正确的 Markdown 规则，但根 `AGENTS.md` 同时混合产品、React、Rust、事务、Provider 和工具循环细节。编码模型必须主动发现并一直记住这些文字，违反边界时仍可能通过编译。`TASKS.md` 也把当前动作与长历史放在同一阅读层级，导致新 Agent 接手时上下文噪声过高。

## 本次变化

- 将编码入口拆为根 `AGENTS.md`、`src/AGENTS.md` 与 `src-tauri/src/AGENTS.md`；根文件负责路由，目录文件负责就近约束。
- 用 `TASKS.md` 的 `ACTIVE_TASKS` 标记建立有界当前任务窗口；历史记录继续保留，但不再冒充当前指令。
- 新增 `.harness/agent-context.json`、`scripts/check-agent-contracts.mjs` 与负向单元测试。
- `agent:check`/pre-commit 现在会拦截：七份代码地图缺失或增生、前端绕过 `local-store.ts` 直接 `invoke`、bridge 调用未注册命令、已注册命令未写 API、外部进程/凭据/HTTP 所有权扩散，以及 Rust/TypeScript/fixture 工具目录漂移。
- 清单相对 Git `HEAD` 只允许收紧；pre-commit 直接执行三份 staged 检查，并拒绝 hook、检查器或配置的部分暂存，避免用 working-tree 强版本放过 index 弱版本。
- 根据 `lib.rs` 的当前注册事实，为 `docs/api.md` 补齐六个既有命令；没有新增或改变 Tauri 命令。

## 行为与数据边界

本次只改变开发者/编码 Agent 上下文、检查脚本和文档。没有改变 React 产品交互、Rust 运行路径、SQLite schema、媒体分析、Agent 工具执行、Provider 选择、preview、Jianying draft 或用户数据。

机器检查只证明可确定的结构和名称约束；它不能证明模型已经理解产品语义。后端校验、范围测试、真实桌面验收和独立 Agent 审查仍是完成门。

## 同步文档

- `AGENTS.md`
- `README.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/harness.md`
- `docs/codebase/STACK.md`
- `docs/codebase/STRUCTURE.md`
- `docs/codebase/ARCHITECTURE.md`
- `docs/codebase/CONVENTIONS.md`
- `docs/codebase/TESTING.md`
- `docs/codebase/CONCERNS.md`

## 验证结果

- `npm run agent:test`、`npm run agent:check` 通过，覆盖嵌套地图、JS/TS/dynamic/alias invoke、静态命令名、裸注册项、API 表、进程/凭据/网络边界、工具目录与清单 ratchet 的正负样例。
- `npm run harness:test`、`npm run harness:check`、`npm run harness:staged` 与 `.githooks/pre-commit` 实际执行通过。
- `npm run lint`、`npm run build` 通过。
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`、`cargo check --manifest-path src-tauri/Cargo.toml` 通过；保留已记录的 `PartiallyDone` dead-code warning。
- `cargo test --manifest-path src-tauri/Cargo.toml` 通过：128 个单元测试、2 个跨模块契约测试。
- Python adapter 14 个测试通过；七份代码地图 exact-file/evidence、`git diff --cached --check` 通过。
- 独立 Agent 审查多轮发现并关闭边界绕过、清单弱化、部分暂存和文档入口失真，最终无 blocker。
