# Phase 4 只精修选中片段，不拼下一段

分支：`cursor/visual-batch-by-segment`。

## 结果

Phase 4 锁定 `segmentId` 后，内容窗停在该段的运动可用区间。片段比旁白短时不再自动并入下一段硬切。召回里已经是 `s001+s002` 的双段候选仍按这两段并窗。

## 范围

- `src-tauri/src/storyboard/phase4.rs`：去掉“不够长就拼下一段”
- `docs/api.md`、`docs/architecture.md`、`docs/decisions.md`

## 禁止变化

- 不改公开 Tauri 命令、40% 上限、第一次 6 段一批
- 不改 Pass B/C 抽帧密度
- 不删无 `segmentId` 时的 Pass A

## 验证

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- storyboard::phase4`
