# 本地 Agent 诊断记录

日期：2026-08-10

用户要求增强当前开发诊断能力。新增独立的本地 SQLite `agent_diagnostics` 表和 `list_agent_diagnostics` 命令；记录以项目、剪辑任务、会话、Agent 调用和可选步骤号严格作用域化，保存受控的模型响应阶段、工具安全错误码与管线失败阶段。它不进入 `agent_run_steps`、`operation_logs` 或应用运行日志，也不会上传。

诊断记录的目标是复盘模型重试和失败根因；模型原文、会话内容、媒体证据、凭据、原始媒体文件和本机路径均不写入记录。`agent_run_steps` 继续保持 payload-free，供通常 UI 审计使用。

同步文档：`README.md`、`docs/architecture.md`、`docs/api.md`、`docs/decisions.md`、`TASKS.md`。

验证：`cargo fmt --check`、`cargo test --lib`、`npm run lint`、`npm run build`、`npm run harness:check`。
