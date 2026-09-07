# 2026-09-07: 短对齐字幕动画不再挡掉配音写入

## 问题

Fish Audio TTS 与素材分析已成功，但 `apply_synthesized_voiceover` 在写入对齐字幕时失败：`Text animation is unsupported or outside supported bounds.`  
原因是 `subtitle_safe` 模板固定 fade 180/160ms，而 alignment cue 常短于该时长；校验失败导致**旁白轨整段不写**，预览无声，Agent 仍可能报已配音。

## 改动

- `apply_text_template` 后钳制/清除超出 cue 时长的动画。
- `subtitle_track_from_cues` 生成时同步钳制。
- 字幕校验失败时仍写入 voiceover，仅跳过对齐字幕并记警告。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml short_subtitle_safe_cues_clamp`
- `cargo test --manifest-path src-tauri/Cargo.toml short_alignment_cues_keep_animation`
- `cargo check --manifest-path src-tauri/Cargo.toml`

## 同步文档

- `docs/api.md`
- `docs/architecture.md`
- `TASKS.md`
