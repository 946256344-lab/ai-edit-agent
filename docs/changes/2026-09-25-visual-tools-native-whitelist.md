# 视觉概览/详情工具补进 NativeToolLoop 白名单

## 背景

`get_library_visual_overview` 与 `get_asset_visual_detail` 已进入工具目录、
`OBSERVATION_TOOLS`、参数解析和执行分发（见
`docs/changes/2026-09-25-visual-overview-detail-tools.md`），但漏加到
`src-tauri/src/agentloop/native.rs` 的 `NATIVE_TOOL_NAMES`。模型能看到这两个工具，
调用时却被执行白名单拒绝；`ordinary_question_returns_message_without_tool_call`
因暴露工具数比白名单多 2 个而失败。

## 变更内容

- `NATIVE_TOOL_NAMES` 在 `list_assets` 之后补入两个观察工具名，顺序与 `OBSERVATION_TOOLS` 一致。
- 其他清单（工具目录、`OBSERVATION_TOOLS`、契约 fixture、TS 镜像类型）已包含两者，无需改动。
- 回归沿用既有测试 `ordinary_question_returns_message_without_tool_call`，未新增测试。

## 影响文档

无公开契约变化。
