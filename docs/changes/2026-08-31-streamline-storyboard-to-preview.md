# 2026-08-31: Storyboard 生成后自动串联时间线与预览

## 问题

Agent `generate_storyboard` 成功后返回 `needs_confirmation`，前端展示 storyboard 确认横幅，用户点击后才调用 `confirm_storyboard_and_preview` 异步创建时间线和预览。Task Resolver 在低置信度时也会打断用户询问“继续当前任务还是创建新任务”，与“自动一路跑到预览”的产品目标不一致。

## 修复方案

- Agent 技能 `generate_storyboard` 成功后立即在同一工具调用内依次执行 `create_timeline_draft` 与 `render_preview`；成功时返回 `status: ok` 并附带 timeline/preview 标识，失败时保留已生成的 storyboard/timeline 并回传安全错误码。
- 前端移除 storyboard 确认横幅与 `confirmStoryboard` 动作；手动成果工作区在 `generateStoryboard` 成功后同样自动串联 timeline 与 preview。
- `local-store.ts` 删除 `confirmStoryboardAndPreview` 封装；后端 Tauri 命令 `confirm_storyboard_and_preview` 仍注册，供历史任务或诊断兼容，但不再是主路径。
- Task Resolver 低置信度时默认 `continue_current`（有激活任务时），不再因低于门槛而 `clarify`；`create_new` 低置信度且存在激活任务时同样回落为继续当前任务。

## 变更范围

**Rust**：
- `agentloop/skills.rs::generate_storyboard` 工具链
- `taskrouter.rs::validate_model_route` 与 `ambiguous_route_result`

**前端**：
- `App.tsx`、`AgentWorkspace.tsx`：移除确认 UI
- `useArtifactWorkspaceController.ts`：手动生成后自动 timeline + preview
- `local-store.ts`：移除 `confirmStoryboardAndPreview`

**不变更**：
- 公开 Tauri 命令签名（含保留的 `confirm_storyboard_and_preview`）
- SQLite schema
- Storyboard 生成与验证逻辑

## 同步文档

触发规则 `desktop-contract` 要求同步：`docs/api.md`。

- [x] `docs/api.md`：`generate_storyboard` 工具行为、Task Resolver 低置信度默认、前端不再调用确认命令。
- [x] 本变更记录。

## 触发规则

- `desktop-contract`：`src/lib/local-store.ts` 删除公开 invoke 封装。

## 验证

- [ ] `cargo test --manifest-path src-tauri/Cargo.toml taskrouter`
- [ ] `npm run branch:check`
- [ ] `npm run harness:test` / `npm run harness:check`
- [ ] `npm run lint` / `npm run build`（前端改动）
