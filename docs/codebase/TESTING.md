# 测试体系

## 1）测试栈与命令

| 范围 | 工具 | 命令 |
| --- | --- | --- |
| TypeScript 静态检查 | Oxlint + `tsc -b` | `npm run lint`、`npm run build` |
| 架构/文档规则 | Node `assert/strict` | `npm run harness:test`、`npm run harness:check` |
| Agent 上下文/跨层契约 | Node `assert/strict` | `npm run agent:test`、`npm run agent:check` |
| Rust 单元/集成 | Cargo 内建 test harness | `cargo test --manifest-path src-tauri/Cargo.toml` |
| Rust 格式 | rustfmt | `cargo fmt --manifest-path src-tauri/Cargo.toml --check` |
| Python adapter | `unittest` + `unittest.mock` | `python -m unittest discover -s src-tauri/scripts -p "test_*.py"` |
| 真实桌面回归 | WebView2 CDP + Node assert | 启动 Tauri 后 `npm run tauri:verify` |

仓库没有统一的 `npm test`、coverage 命令或 CI pipeline。

## 2）测试布局

- Rust 单元测试与实现共置：`src-tauri/src/*.rs` 内的 `#[cfg(test)] mod tests`。
- 跨模块契约：`src-tauri/tests/agent_contract_assets.rs`。
- Agent fixture：`src-tauri/tests/fixtures/*.v1.json`，白名单变化必须同步 fixture。
- Python：`src-tauri/scripts/test_create_jianying_draft.py`。
- 开发 harness：`scripts/test-architecture-budgets.mjs`、`scripts/test-agent-contracts.mjs`、`scripts/test-doc-sync.mjs`。
- 桌面 smoke：`scripts/verify-tauri-webview.mjs`，要求真实 Tauri/WebView 和已有本地项目。

## 3）范围矩阵

| 范围 | 覆盖 | 典型目标 | 说明 |
| --- | --- | --- | --- |
| 纯函数单元 | 是 | 路径、时间范围、文本、目标/策略、序列化 | Rust 共置测试 |
| SQLite 集成 | 是 | 迁移、作用域、receipt、终态事务 | 多数使用临时连接/fixture |
| 外部适配器隔离 | 是 | Jianying Python 生成/回滚/注册 | 使用 mock 和临时目录 |
| Agent 契约一致性 | 部分 | 21 技能白名单和风险 fixture | 只验证 catalog/脚本结构 |
| 完整模型循环 | 否 | 多轮 provider script → SQLite/产物 | fixture README 明确未实现 |
| React 组件单元 | 否 | hooks、工作区、目录树 | 未安装 Vitest/Jest/RTL |
| 桌面 E2E | 部分 | 启动、模式、目录、Provider 模态 | CDP smoke，不执行生成副作用 |
| 媒体/Jianying 实机 | 人工证据 | preview 播放、Jianying 兼容 | 记录在 TASKS/changes，非自动 CI |

## 4）Mock 与隔离

- Rust 测试主要把验证逻辑作为纯函数调用，数据库测试构造临时 SQLite 状态。
- Agent contract test 直接读取 `agentloop/policy.rs` 的工具常量并与 JSON fixture 对账，防止白名单漂移。
- Python 使用 `patch` 替换 `DraftFolder`、进程查询和注册函数，并使用 `TemporaryDirectory`。
- WebView 脚本不 mock Tauri；它连接真实桌面窗口，但刻意不发送 Agent 请求或创建产物。

## 5）质量信号与缺口

- 架构预算和文档同步已进入本地 pre-commit。
- 分层 Agent 指令、源码顶部中文职责导航、IPC/注册/API 对账、进程/凭据/HTTP 所有权和工具目录一致性也进入同一 pre-commit；配置相对 `HEAD` 只允许收紧，staged 运行文件必须完整暂存。它只覆盖可机器判定的结构事实，不能判断注释语义是否准确。
- 没有覆盖率工具、覆盖率阈值或当前覆盖率报告。[TODO]
- 没有 GitHub Actions/其他 CI；检查依赖开发者本机 hook。[TODO]
- 完整 Agent 多步 transcript、事件主动丢失、worker 崩溃恢复没有可重复的全自动 runner。
- `tauri:verify` 依赖已有包含 891 条素材的本机项目，不是自包含 fixture。
- 当前测试数量应以实际 `cargo test`/Python 输出为准，不在本文写死，避免随新增测试失真。

## 6）建议新增顺序

1. 为 Agent loop 引入仅测试可用的 scripted decision seam，执行现有风险 fixture。
2. 为 task receipt + conversation finalization 建立跨模块事务回归。
3. 为素材目录投影和分析 worker 抽取后的模块保持现有测试搬迁，不重写断言。
4. 增加最小 React hook 测试，重点覆盖项目切换竞态和终态对账。
5. 最后接入 Windows CI；媒体/Jianying 仍需分离为环境能力测试。

## 7）证据

- `package.json`
- `src-tauri/tests/agent_contract_assets.rs`
- `src-tauri/tests/fixtures/README.md`
- `src-tauri/scripts/test_create_jianying_draft.py`
- `scripts/verify-tauri-webview.mjs`
- `.githooks/pre-commit`
- `.harness/agent-context.json`
