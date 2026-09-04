# 2026-09-04：对话「处理中」可停止

## 变更

Composer 在处理中显示可点击的「停止」，调用 `cancel_agent_edit` 将 queued/running 任务标为 `cancelled`。NativeToolLoop 在下一步检查点退出，终态与回复诚实保留已确认产物。

## 文件

- `src-tauri/src/agent.rs`：`cancel_agent_edit`；queued→running 认领；终态允许 `cancelled`
- `src-tauri/src/agentloop/schema.rs` / `native.rs`：Cancelled 终态文案
- `src/components/AgentWorkspace.tsx`、`App.tsx`、`AgentRunCard.tsx`、`local-store.ts`
- `docs/api.md`

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml`
- `npm run build`
- `npm run harness:check`
