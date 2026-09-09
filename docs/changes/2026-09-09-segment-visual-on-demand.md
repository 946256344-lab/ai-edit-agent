# 片段级按需视觉与向量

## 结果

新增 `analyze_asset_segments_batch`：对被粗召回/检索命中的素材按需跑片段视觉并永久缓存；`asset_segment_embeddings` 表存片段向量；`ensure_segment_visual_evidence` 带预算等待。

## 范围

- 新增 `src-tauri/src/assets/segment_visual.rs`
- 改 `src-tauri/src/assets/visual.rs` worker 兼容双任务
- 改 `src-tauri/src/storyboard/semantic.rs`（`EMBEDDING_VERSION=3`、片段向量）
- `SCHEMA_VERSION` 15→16；`search_asset_segments` 返回 `segmentId`/`shotType`
- 同步 `docs/api.md`、`docs/architecture.md`

## 禁止变化

- 新导入仍走素材级 1 帧视觉（粗召回）
- 不做整库片段视觉预跑

## 契约

- 新任务名 `analyze_asset_segments_batch`
- `visualAnalysisVersion=2` 表示片段证据齐全
- `search_asset_segments` 结果含 `segmentId`、`shotType`
