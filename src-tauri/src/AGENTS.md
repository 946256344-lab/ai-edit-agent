# Rust 后端 Agent 规则

本文件适用于 `src-tauri/src/`。开始前先读仓库根 `AGENTS.md`、`TASKS.md` 当前窗口和 `docs/codebase/ARCHITECTURE.md`。按范围补读 `docs/api.md`、`docs/codebase/INTEGRATIONS.md`、`docs/codebase/TESTING.md` 与 `docs/codebase/CONCERNS.md`。

## 可信边界

- React 只提交意图；Rust 校验 project、editing task、conversation、storyboard、timeline、asset 和路径作用域，并决定真实副作用与完成状态。
- 用户消息进入 conversation 或产生副作用前，必须先由 Task Resolver 确定归属并签发一次性 route receipt；Resolver 只选任务，不选工具。
- Conversation Router 的执行型首轮决定必须复用为 Agent loop step 1。Agent loop 封闭、有界，工具参数在 JSON 顶层，完成门由真实产物验证。
- 只读请求禁用编辑和交付工具；用户明确排除的 preview、Jianying draft 或素材分析必须同时从路由、目标与技能策略中排除。
- 观察工具不得暗中触发分析；编辑与交付必须是具名、作用域化、可审计的工具调用。未知中断不得自动重放副作用。

## 持久化与恢复

- Agent task 终态、可选产物审计、确定性最终回复和 conversation 终态在同一 SQLite 事务提交；提交成功后才发送 `agent-edit-completed`。
- 启动恢复不得猜测丢失的模型回答。`working` conversation 的终态 task 缺少确定性回复时转 `needs_review` 并写固定恢复消息。
- 迁移只追加、可重复执行，不删除或覆盖用户数据。新增/修改 schema、事务或恢复状态必须同步 `docs/architecture.md`、`docs/api.md`、`docs/decisions.md` 和测试。
- 不得用旧 Agent task result 否定更新的 storyboard、timeline 或磁盘 preview 事实。

## 外部边界

- 所有 Windows 外部进程通过 `process::hidden_command` 创建，不得在业务模块直接 `Command::new`。素材分析已有硬超时；preview/Jianying 的部分同步交付调用仍无超时，是已知债务。新增或修改长运行调用时必须补超时/取消边界或记录明确 `TODO`，不得声称现状已全部覆盖。
- Credential Manager 访问只属于 `oauth.rs`、`custom_api.rs` 和 `music_provider.rs`。模型 Provider 凭据必须区分不存在与读取失败，读取失败时封闭；Jamendo 当前仍把两者都投影为 `disconnected`，这是已知债务，不得把它写成已解决。
- HTTP/网络传输只属于 `provider.rs`、`oauth.rs` 和 `music_provider.rs`；模型 Provider 选择统一经 `ModelAccess`。
- 日志、诊断、错误和审计不得包含 prompt、模型响应原文、会话内容、媒体证据、凭据或完整本机路径。
- Jianying 只创建唯一新 draft；不得覆盖或反向同步。

## 模块演进

- `lib.rs` 是模块索引和 Tauri 命令注册事实来源；公开命令必须同步 `src/lib/local-store.ts`（如有 UI 调用）和 `docs/api.md`。
- `agentloop.rs` 与 `assets.rs` 是受预算保护的热点。Agent 的纯请求策略、工具白名单和真实产物完成门已进入 `agentloop/policy.rs`；继续拆分前按 `docs/codebase/CONCERNS.md` 的顺序迁移 router/state/executor，素材侧先抽 library，不得提高预算掩盖增长。
- Agent 工具白名单、`src/lib/agent-tools.ts` 和 `src-tauri/tests/fixtures/agent_tool_contracts.v1.json` 必须保持一致。
- 注释优先说明作用域、事务顺序、幂等性、恢复和隐私理由。

## 修改完成

运行：

```powershell
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run agent:check
npm run harness:check
python -m unittest discover -s src-tauri/scripts -p "test_*.py"
```

涉及高风险运行链路时，还要按 `docs/harness.md` 执行对应 fixture、真实桌面验收和独立 Agent 审查。
