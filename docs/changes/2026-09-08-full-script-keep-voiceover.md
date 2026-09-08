# 2026-09-08: full_script 重建时间线保留配音

## 问题

可念稿 brief 被纠偏为 `full_script` 并完成 audio-first 配音后，收尾缺口把 500ms 混音尾当成「画面短于旁白」，Agent 再调 `create_timeline_draft` 得到一条无旁白、只有标记字幕的时间线，预览听起来像 key_message。

## 改动

- `storyboard_completion_gaps` 按口播时钟（`target - VOICEOVER_TAIL_MS`）判断缺口。
- `create_timeline_draft` 继承同一 storyboard 已有旁白轨与 `voice_alignment` 字幕。
- Agent `create_timeline_draft` 在无旁白时仍走自动配音（缓存命中则很快）。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml --lib completion_gaps`

## 同步文档

- `docs/api.md`、`TASKS.md`
