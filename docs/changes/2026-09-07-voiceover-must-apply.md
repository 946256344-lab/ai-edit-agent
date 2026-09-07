# 2026-09-07: 配音旁白必写 + Fish 传输回退

## 问题

1. alignment 不完整时 `synthesize_voiceover_for_timeline` 直接 `Err`，已合成音频不落时间线。
2. Fish 配置后传输失败（unavailable/timeout）不回退 ElevenLabs，整段无声。
3. 快照「配音=已配置」易被模型当成「已配音」；cue.provider 写死 ElevenLabs。

## 改动

- 旁白写入与字幕解耦：alignment/字幕失败只 warning，仍 `apply` voiceover。
- `synthesize_with_fallback`：优先 Fish，传输类错误回退 ElevenLabs（401/密钥不回退；回退时清空 Fish 音色 ID）。
- `VoiceoverApplyResult` 增加 `voiceoverApplied` / `subtitleApplied` / `provider`；工具与自动配音文案按事实报告。
- 快照：`voiceoverCues` + `配音能力`；设置文案同步。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml fallback_eligible_only_for_transport`
- `cargo test --manifest-path src-tauri/Cargo.toml snapshot`
- `cargo check --manifest-path src-tauri/Cargo.toml`

## 同步文档

- `docs/api.md`
- `docs/architecture.md`
- `TASKS.md`
