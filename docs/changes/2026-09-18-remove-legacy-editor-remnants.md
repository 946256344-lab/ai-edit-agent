# 2026-09-18：去掉输出端口落地后的旧残留

## 结果

工作台只保留粗剪预览与顶栏输出端口。未挂载的成果页、前端剪映包装和重复的素材采集已删除。剪映仍走链接器；`create_jianying_draft` 仍可作为强制剪映命令。

## 范围

- 删除 `ArtifactsWorkspace.tsx`、`AgentAuditPanel.tsx` 及对应暗色成果页样式
- 前端交付只调用 `deliver_to_editor`；去掉未使用的 `createJianyingDraft` / 手动分镜包装
- `jianying.rs` 复用 `handoff/deliver` 的源采集
- `docs/architecture.md`、`docs/codebase/`、`.harness/architecture-budgets.json`

## 禁止变化

- 不改公开 Tauri 命令名
- 不覆盖已有草稿
- 不实现 CapCut

## 验证

- `npm run lint`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `npm run harness:check`
