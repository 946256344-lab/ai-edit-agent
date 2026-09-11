# Phase 4 局部修复合入 clip-segment-recall（step 1）

分支：`cursor/clip-segment-recall`。

## 改动

将主仓 `54c608b` 的 Phase 4 局部修复接到本分支：`Phase4Session` 在当次调用内保留成功 Pass B/C 批次；外层循环传入 `&mut session`；`phases::phase4_refine_ranges` 改为薄包装调用 `storyboard/phase4.rs`。CLIP 图文加权、库存感知 Phase 1、Phase 2 shortlist / Phase 3 相似硬拒均未回退。尚未改 `extract_frames_at_times`（step 2）。

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml --lib` 通过。
- `cargo test --manifest-path src-tauri/Cargo.toml --lib phase4::`：9 项通过。
