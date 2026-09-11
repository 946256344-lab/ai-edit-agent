# Phase 4：批次续跑上线 + 一镜一次抽帧

## 结果

1. 将 `Phase4Session`（成功批次保留、失败批次续跑）接到当前 CLIP/库存写 beat 工作树，运行中的 Phase 4 重试不再整段重做已成功的 Pass B/C。
2. `extract_frames_at_times` 改为同一时间窗只启动一次 FFmpeg（`-ss` 到窗首 + `select` 命中各目标时刻），文件名仍带真实 `time_ms`；批量失败再按帧回退。

## 范围

- 合入/接线：`storyboard/phase4.rs`、`storyboard.rs`、`phases.rs`、`timing.rs`、`repair.rs`、`shot_replacement.rs`
- 抽帧：`storyboard/multimodal.rs`
- 文档：`TASKS.md`、`docs/architecture.md`、`docs/changes/`

## 禁止变化

- 不降低 Pass B/C 每镜抽帧密度
- 不改公开命令 / schema
- 不回退 CLIP 与 Phase 1 库存摘要

## 验证

- `cargo check --lib`
- `cargo test --lib phase4::` / `pts_select_expression` / scoped timing-overlap
