# 2026-09-03: 音频优先 + 禁止冻结帧补时长

## 触发范围

`src-tauri/src/timeline.rs`（`insert_clips`）、`timeline_voice.rs`、`storyboard.rs`（audio-first finalize）、`voice_provider.rs`、`agentloop/{tools,skills,policy,native}.rs`、契约 fixture、`docs/api.md`、本记录。

## 改动

### B：音频优先主链路（收尾）
- `full_script` 在 Phase 1 后 `prepare_audio_first`，用真实 TTS 时长覆盖 `target_duration_ms`。
- `finalize_audio_first_timeline`：画面 ≥ 配音时写入 voiceover + alignment 字幕；画面不足时只落画面时间线，不挂不匹配配音，留给 `insert_clips` 修复。
- Agent `generate_storyboard` 优先复用该 storyboard 已写入的 audio-first 时间线，避免再造一条无声 draft；可选 `voiceId`。

### A：禁冻结 + 模型补时长
- `fit_visual_to_voiceover` 继续拒绝 `freeze_frame`，错误码 `voiceover_longer_than_picture`（分号分隔字段便于诊断解析）。
- 新增写工具 `insert_clips`：在已验证源范围内插入视频/图片镜头，`fit_reason=timeline_extend`，生成新 timeline version。
- `safe_tool_failure_context` 对 deficit 给出可重试恢复指引（搜段 → insert/change/replace → 再 synthesize）。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml`（含 `insert_clips` 与 voiceover fit 回归）
- `cargo test --test agent_contract_assets --manifest-path src-tauri/Cargo.toml`
- `npm run harness:check`

## 决策

无 ADR。沿用：Rust 为时间线可信边界；口播为时钟；禁止用冻结帧掩盖时长缺口。
