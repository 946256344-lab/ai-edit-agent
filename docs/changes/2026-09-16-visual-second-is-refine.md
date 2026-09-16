# 第二次分析改为选中镜头精修

分支：`cursor/visual-batch-by-segment`。

## 结果

选片和片段检索不再排队或等待 `analyze_asset_segments_batch`。第一次识别回写段卡时写入片段向量；补跑按段上有卡，不要求 `visualAnalysisVersion>=2`。第二次分析就是选中镜头的 Phase 4 多帧网格精修，判断动作做完没有、后半截有没有新信息。历史加深任务仍可由 worker 收尾。

## 范围

- `src-tauri/src/assets/segment_visual.rs`：ensure 只补本地向量，不跑模型
- `src-tauri/src/assets/visual.rs`：第一次回写时写片段向量
- `src-tauri/src/storyboard/semantic.rs`：补跑不再要求 version=2
- `src-tauri/src/storyboard/phase4.rs`：精修网格提示
- `docs/api.md`、`docs/architecture.md`、`docs/decisions.md`

## 禁止变化

- 不整库预跑视觉模型
- 不加 `coverage` 字段、不改每 beat 最少 2 镜
- 不改公开 Tauri 命令、超时或第一次 6 段一批

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- assets::visual assets::segment_visual storyboard::semantic`
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
