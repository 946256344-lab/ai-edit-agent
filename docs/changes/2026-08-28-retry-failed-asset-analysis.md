# Agent 工具：重试失败素材分析

日期：2026-08-28

## 目标

让 NativeToolLoop 能重试批量自动分析中失败的技术和/或视觉阶段，无需模型手动收集失败素材 ID。

## 变更

- 新增 Agent 写工具 `retry_failed_asset_analysis`：`stage` 可选 `technical` / `visual` / `both`（默认 both）；`assetIds=null` 时自动收集当前项目最多 200 条失败素材；显式 `assetIds` 只重试仍处于失败状态的项。
- Rust 核心逻辑位于 `assets/analysis.rs`：技术失败走既有 `request_asset_analysis`，视觉失败走既有 `queue_visual_analysis_batch`；排除禁止使用素材、源文件不可用和用户显式跳过的视觉分析。
- 同步 `agentloop/tools.rs`、`policy.rs`、`skills.rs`、`native.rs` 参数校验与结果验真；契约 fixture、`agent-tools.ts`、`AgentRunCard` 标签与 `docs/api.md`。

## 不变边界

- 不新增公开 Tauri 命令；UI 仍可使用既有 `retry_asset_analysis_batch`（仅技术分析）。
- 不改变 SQLite schema、Provider 协议或后台 worker 并发上限。

## 验证

- `cargo test retry_failed main_chain_arguments native_write_catalog`
- `npm run harness:check`
