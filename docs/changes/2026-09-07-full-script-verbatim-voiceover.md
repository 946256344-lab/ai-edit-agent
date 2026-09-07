# 2026-09-07: full_script 照念原文

## 问题

Phase 1 模型把完整文案改写成 `beats[].narration` 后，audio-first TTS 拼接 beats 合成，口播与用户原文不一致。

## 改动

- Phase 1 要求模型识别完整文案：`scriptMode=full_script` + `spokenScript`（原文，不改写）；beat.narration 只做同文拆分。
- `resolve_voiceover_script`：full_script 优先 `spokenScript`，否则大段 brief；不用改写 beats。
- audio-first / `storyboard_narration_text` 共用该解析。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml full_script_prefers_spoken --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml storyboard_narration_full_script --lib`
- `cargo test --manifest-path src-tauri/Cargo.toml speakable_brief_skips --lib`

## 同步文档

- `docs/api.md`
- `docs/architecture.md`
- `TASKS.md`
