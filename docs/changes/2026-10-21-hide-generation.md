# 隐藏应用内生成并打包 MCP（2026-10-21）

定位改为素材整理 + 外部 Agent 剪辑后，界面默认不再进入对话生成；Release 安装包自动带上 `voycut-mcp.exe`。

## 触发范围

- `src/App.tsx`、`src/components/AppSidebar.tsx`、`src/components/WorkspaceHeader.tsx`、`src/components/AssetManagementPanel.tsx`、`src/components/asset-workspace/*`、`src/components/McpConnectPanel.tsx`、`src/components/ProjectSettingsModal.tsx`
- `src/lib/product-mode.ts`、`src/lib/local-store.ts`、`src/lib/i18n/{zh-CN,en}.ts`、`src/hooks/useAssetWorkspaceController.ts`、`src/hooks/useMcpConnectController.ts`
- `src-tauri/src/lib.rs`、`src-tauri/src/mcp.rs`、`src-tauri/src/process.rs`、`src-tauri/src/jianying.rs`、`scripts/run-tauri.mjs`（sidecar 通过 `--config` 注入 `externalBin`，不改 `tauri.conf.json`）
- 文档：`docs/api.md`、`docs/architecture.md`、`README.md`、`docs/codebase/STRUCTURE.md`

未改 `docs/decisions.md`、`TASKS.md`、方案文件；未删后端生成命令；尽量未改 `models.rs`、`storyboard.rs`、`storyboard/phases.rs`、`phase4.rs`。

## 改动

- 集中开关 `IN_APP_GENERATION_ENABLED=false`：输入框、对话、体裁、故事版、时间线、预览、配音、交付和剪辑会话在界面上不可见、不可触发；改 `true` 可恢复。删除项目/素材仍先确认。
- 主路径变为素材库：检索、查看片段证据（风险三态 hit / not_hit / unknown）和「接入 Claude Code / Codex」说明；导入不再为了落一条对话而新建剪辑会话。
- 新增 `get_mcp_install_info`：返回本机 `voycut-mcp.exe` 路径和可复制的 `claude mcp add` / Codex 片段。
- `scripts/run-tauri.mjs` 构建 sidecar 并以 Tauri `--config` 注入 `bundle.externalBin`（不写进 `tauri.conf.json`，以免未构建 sidecar 时 `cargo check` 失败）；MCP/剪映适配按可执行文件目录查找随包 Python 与脚本，不必设 `PYTHON_PATH`。

## 同步文档

`docs/api.md`、`docs/architecture.md`、`README.md`、`docs/codebase/STRUCTURE.md`、本记录。

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml` 通过（保留既有未接线模块的 dead_code 警告）。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib mcp::tests`：路径引用片段 1 项通过。
- `npx oxlint@1.75.0 -c .oxlintrc.json src` 通过。本 worktree 与主仓库 `node_modules` 均为空，未 `npm install`，因此 `npm run lint` 找不到本地 oxlint。
- `npm run harness:check`、`git diff --check` 通过。
- 隔离目录 `VOYCUT_APP_DATA_DIR=.mcp-verify/lock-test`：两个 `voycut-mcp` 同时打开，`list_projects` 与失败的 FCPXML 交付（项目不存在）返回真实错误，无 database locked / 损坏；`pragma integrity_check=ok`。未启动桌面 GUI，未对真实素材跑通 FCPXML 写出（评测快照目录不存在）。

决策：无（沿用 2026-10-21 产品方向，不改 ADR）。
