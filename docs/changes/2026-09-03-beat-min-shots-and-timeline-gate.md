# 2026-09-03: 每 beat 至少两镜 + 时间线收尾检查

## 问题

重新剪辑时 Phase 3 只是“可以”拆镜，多数 beat 仍一镜带过；audio-first 画面短于旁白时只打 WARN 并以 `status=ok` 结束，Agent 不再补镜，造成假完成。

## 改动

- Phase 3 prompt：每个有候选备选的已覆盖 beat **必须**拆成 2–3 连续镜。
- `collect_phase3_issues`：候选池 ≥2 时硬拒绝 `beat_below_min_shots`；池仅 1 个时记 soft issue，留给收尾。
- `storyboard_completion_gaps`：检查 uncovered beats、每 covered beat 镜数、full_script 画面相对 `targetDurationMs` 缺口。
- `generate_storyboard` 工具结果附带 `qualityWarnings`；有缺口时推迟 preview，依赖精炼续步走 `insert_clips` / `change_clip_duration` / `replace_clips`。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml storyboard::`
- `cargo test --manifest-path src-tauri/Cargo.toml agentloop::`
- `npm run harness:check`（提交前）
