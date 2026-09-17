# 配音先于拆拍，时间戳对齐真实口播

分支：`feature/agent-led-storyboard`（PR2）。

## 结果

配音开启时先按已确认旁白稿合成，再让模型拆拍。合成失败不开始生成。用户点名了成片秒数且与口播相差超过约 30% 时先问用户。Fish 有词级时间戳就用词级；只有句级片段时按字数插值，让拍可以切在句中。

## 范围

- `src-tauri/src/storyboard.rs`：TTS 移到 Phase 1 前；失败封闭；`requestedDurationMs` 冲突走 `storyboard_needs_user_decision`
- `src-tauri/src/storyboard/phases.rs`：Phase 1 注入真实口播时长
- `src-tauri/src/storyboard/timing.rs`：词级优先，句级按字插值
- `src-tauri/src/music_provider.rs`：Fish 流式 alignment 保留 `words`
- `src-tauri/src/agentloop/tools.rs`、`native.rs`、`skills.rs`：新增 `requestedDurationMs`；配音失败本轮不再重跑

## 禁止变化

- 不改公开 Tauri 命令名和参数
- 不改 40% 上限、相似硬拒、第一次 6 段一批
- 不在一拍里机械补第 2 镜
