# 补全 get_library_visual_overview 与 get_asset_visual_detail 工具契约

## 背景

`get_library_visual_overview` 和 `get_asset_visual_detail` 已在
`src-tauri/src/agentloop/policy.rs`、`tools.rs`、`skills.rs`、`native.rs` 完整实现，
但 TypeScript 镜像类型和版本化 fixture 未同步，导致 harness 契约检查失败。

## 变更内容

- `src/lib/agent-tools.ts`：`AgentObservationToolName` 新增两个工具名。
- `src-tauri/tests/fixtures/agent_tool_contracts.v1.json`：在 `list_assets` 之后补入两个观察工具条目，`kind` 均为 `"observation"`。
- `docs/api.md`：在 Agent 工具表 `list_assets` 行之后补入两行描述，与 Rust 实现一致。

## 影响文档

- docs/api.md

## 工具语义

`get_library_visual_overview`：聚合当前项目全部就绪素材的持久化视觉证据（主体、动作、场景、字幕、叙事角色），在写文案或规划分镜前调用，不访问源文件。

`get_asset_visual_detail`：返回单条素材完整片段级视觉证据（场景、主体、动作、字幕、叙事角色、镜头类型、摄像机运动、可用时间范围），`assetId` 须属于当前项目且已就绪，不返回路径。
