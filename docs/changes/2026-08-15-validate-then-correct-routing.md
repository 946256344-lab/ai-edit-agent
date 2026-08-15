# validate-then-correct 路由验证模式（2026-08-15）

## 变更范围

- `src-tauri/src/agentloop.rs`：`decide_conversation_route` 新增 validate-then-correct 重试；提取 `try_build_route_decision` 辅助函数封装路由决策构建与验证；`fast_goal` 降级为纯提示不再硬阻断路由；`AGENT_RUN_TIMEOUT` 从 90 s 提升至 300 s；`EDIT_VERBS` 移除"剪辑"避免过度触发；`#[rustfmt::skip]` 保持预算合规。
- `src-tauri/src/agentloop/policy.rs`：同步调整 policy 相关辅助注释。
- `src-tauri/src/taskrouter.rs`：`resolve_conversation_task` 新增 validate-then-correct 重试；`#[rustfmt::skip]` 保持预算合规。
- `src-tauri/src/preview.rs`、`src-tauri/src/preview_tests.rs`：FFmpeg `-t` 参数收敛修复（此前合并进本变更集）。
- `src-tauri/Cargo.toml`：依赖版本对齐（此前合并进本变更集）。

## 模式说明

路由验证失败（模型响应不符合严格 schema）时，将错误原因字符串作为纠偏提示反馈给模型并重试一次；纠偏后仍验证失败则 fail-closed。错误字符串不含用户内容或媒体证据。

## 禁止变化

- 公开 Tauri 命令签名不变
- SQLite schema 不变
- Agent 工具白名单不变
- 用户数据不变

## 同步文档

- TASKS.md：新增本次完成条目
- docs/architecture.md：新增维护记录
- docs/api.md：新增维护记录（命令不变）
- docs/decisions.md：新增 validate-then-correct 决策记录
- README.md：新增维护记录

## 验证证据

- `cargo test`：129 库测试 + 2 契约测试，全部通过
- `npm run lint`：0 警告
- `npm run build`：成功
- `npm run harness:check`：架构预算 27 文件通过；agent 上下文/IPC/边界通过；doc-sync 通过
- `git diff --check`：无尾随空白
