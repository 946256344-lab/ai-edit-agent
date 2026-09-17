# P3 后短窗交给选片模型，不再整单拉窗失败

分支：`feature/agent-led-storyboard`（PR1）。

## 结果

Phase 3 选出镜头后，Rust 用该段运动可用窗对照该拍旁白/节奏时长。窗不够长时把事实交回选片模型：从前 5 条换一条，或只把词拨到上一拍/下一拍一次。仍不够则返回 `storyboard_needs_user_decision`，不落库，Agent 问用户。Phase 4 不再把 `beat_audio_window_shortfall` 当成「把现有窗拉长」的精修失败。

## 范围

- `src-tauri/src/storyboard/length.rs`：可用窗对照、拨词校验、用户决策错误
- `src-tauri/src/storyboard/phases.rs`：候选卡加 `usableMs`/`narrationMs`；选片可回写邻拍 `narration`
- `src-tauri/src/storyboard.rs`：P3 循环在进 P4 前处理短窗
- `src-tauri/src/storyboard/timing.rs`：短窗不再要求模型拉窗；画面短于口播不再挡 Phase 5
- `src-tauri/src/agentloop/skills.rs`、`native.rs`：新错误码，本轮不再重跑 generate

## 禁止变化

- 不改公开 Tauri 命令、40% 上限、相似硬拒、第一次 6 段一批
- 不在一拍里机械补第 2 镜
- 不拼下一段硬切
- Phase 4 仍禁止换片，只改切点

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- storyboard::length storyboard::timing agentloop::native::tests::short_window`
