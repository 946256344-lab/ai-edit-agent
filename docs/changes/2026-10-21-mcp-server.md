# 本地 MCP 服务（2026-10-21）

外部 Agent（Claude Code / Codex）通过 stdio MCP 使用本机已分析素材库，并按镜头清单新建剪映 / CapCut / FCPXML / OTIO 草稿。对应产品方向见 `docs/decisions.md`「素材整理 + 外部 Agent 剪辑」。

## 触发范围

- `src-tauri/src/lib.rs`、`src-tauri/Cargo.toml`、`src-tauri/src/bin/voycut_mcp.rs`、`src-tauri/src/mcp.rs`、`src-tauri/src/mcp/{protocol,tools}.rs`
- `src-tauri/src/assets.rs`、`src-tauri/src/assets/library.rs`（抽出 `load_asset_evidence`，桌面 `get_asset_evidence` 仍授权 WebView）
- 文档：`README.md`、`docs/api.md`、`docs/architecture.md`、`docs/codebase/STRUCTURE.md`

未改 `docs/decisions.md`、`TASKS.md`、方案文件；未改 `models.rs`、`storyboard.rs`、`storyboard/phases.rs`、`phase4.rs`、时间线与交付实现（MCP 只调用现有 `insert_storyboard_version`、`create_timeline_draft_with_options`、`deliver_to_editor`）。

## 改动

- 新增无窗口二进制 `voycut-mcp`：newline JSON-RPC MCP，stdout 只发协议，日志走 stderr。
- 工具：列出项目；列出/检索素材（返回片段、源窗、画面描述、风险三态）；取单条素材证据与关键帧路径；按镜头清单新建时间线并交付指定编辑器端口。
- 与桌面共用 SQLite（WAL）；`VOYCUT_APP_DATA_DIR` 仅用于隔离验收。不提供分析工具，不读凭据。
- 空清单、不存在的素材、越界时间返回真实错误；交付沿用现有唯一草稿名 / `unique_path`，只新建不覆盖。

## 同步文档

`README.md`、`docs/api.md`、`docs/architecture.md`、`docs/codebase/STRUCTURE.md`、本记录。

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml --bins --lib` 通过（保留既有未接线模块的 dead_code 警告）。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib mcp::`：4 项镜头校验通过（空窗、越界、未知片段、合法窗）。
- `npm run harness:check`、`git diff --check` 通过。未改前端，未跑 lint。
- 脚本化 stdio 会话（隔离副本，不写用户真实应用库）：检索 `road` → 取证据 → 交付 FCPXML 成功；空清单 / 不存在素材 / 越界时间均 `isError` 并返回真实原因。剪映、CapCut 草稿目录已写出且 `registered`；未在编辑器 GUI 里打开确认。

决策：无（沿用 2026-10-21 产品方向，不改 ADR）。
