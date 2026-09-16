# Phase 2 直接按段取 9 条

分支：`cursor/visual-batch-by-segment`。

## 结果

Phase 2 不再先锁 9 条整片再展开。就绪视频全部按硬切段进入排序（无硬切则整条 1 段），每个 beat 取 9 段。同片最多 2 段；长得像的最多 2 条。有 1 条就能覆盖该 beat。后面 beat 不再因为前面池子里没用上的相似段被预删。

## 范围

- `src-tauri/src/storyboard.rs`：展开全部就绪视频后选段
- `src-tauri/src/storyboard/phases.rs`：9 段池、相似上限、1 段可覆盖
- `src-tauri/src/shot_replacement.rs`：沿用同一选段
- `docs/api.md`、`docs/architecture.md`、`docs/decisions.md`

## 禁止变化

- 不改 Phase 3 选片契约、40% 复用、第一次分析 65 秒等待
- 不改公开 Tauri 命令

## 验证

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- storyboard::phases storyboard::tests::storyboard_sources`
