# 2026-09-08: key_message 改为屏幕标记、不自动配音

## 问题

产品定义里 `key_message` 只需屏幕标记字幕，不需要口播；旧链路仍为 key_message 写旁白、跑旁白硬门，并在时间线后自动配音，且对齐字幕会覆盖 storyboard 标记。

## 改动

- `StoryboardBeat.onScreenText`（serde default）；前端 `beats.onScreenText?`。
- Phase 1：key_message 写 ≤24 字标记、`narration=""`；`key_message_marker_issue` 做标记与可读性下限门禁。
- `SpeechTiming.kind=Pacing` + `from_pacing_plan`；key_message 在 P2/P3 后写入节奏计划，P4 `fit_shots` / P5 validate。
- normalize：key_message lead 写 beat 标记，不从 purpose 回填旁白。
- 时间线：key_message 每 beat 一条 `beat-<id>-marker` cue；`auto_synthesize_storyboard_voiceover` 跳过 key_message；Agent 文案提示「不配音，已写入字幕标记」。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml --lib storyboard`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib pacing_plan`

## 同步文档

- `docs/api.md`、`docs/architecture.md`、`docs/decisions.md`、`TASKS.md`
