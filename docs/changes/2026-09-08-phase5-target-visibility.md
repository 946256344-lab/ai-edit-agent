# 2026-09-08: Phase 5 保留目标时长可见性

## 问题

`normalize` 把 `targetDurationMs` 改写成镜头总和，并把过短的 `full_script` 降级为 `key_message`，导致 `storyboard_completion_gaps` 的 `voiceover_longer_than_picture` 几乎永不触发；同时 validate 用对称容差拦画面不足，与「缺口走精炼补镜」冲突。

## 改动

- `normalize_storyboard_candidate`：不再改写 `targetDurationMs`，不再降级 `scriptMode`。
- `validate_storyboard`：`key_message` 对称校验总时长；`full_script` 只拦画面超过目标+容差；删除「too short for full-script narration」硬失败，保留镜数下限。
- fixture：`normalize_drops_uncovered_ids_that_already_have_shots` 断言 target/scriptMode 保留。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml --lib storyboard`

## 同步文档

- `docs/api.md`、`docs/architecture.md`、`TASKS.md`
