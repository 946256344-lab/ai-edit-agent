# 覆盖 beat 允许一镜

分支：`cursor/visual-batch-by-segment`。

## 结果

Phase 3 不再把「每 beat 至少 2 镜」当作完成门。有第二条不相似、又对得上文案的候选可以加到 2–3 镜；没有就一镜过关。校验不再报 `beat_below_min_shots`。收尾 `qualityWarnings` 不再因只有 1 镜告警；覆盖 beat 若 0 镜仍报 `covered_beat_without_shots`。每 beat 仍最多 3 镜。

## 范围

- `src-tauri/src/storyboard/phases.rs`：prompt 与 `collect_phase3_issues`
- `src-tauri/src/storyboard.rs`：`storyboard_completion_gaps`
- `src-tauri/src/agentloop/tools.rs`：生成分镜工具说明
- `docs/api.md`、`docs/architecture.md`、`docs/decisions.md`

## 禁止变化

- 不改 40% 复用上限、相似硬拒、同 beat 禁同片
- 不改公开 Tauri 命令

## 验证

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- storyboard::phases storyboard::tests::completion_gaps storyboard::step_retry`
