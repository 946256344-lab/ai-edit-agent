# CLIP 片段图文召回

## 结果

Phase 2 评分新增 CLIP 图文权重（0–25）：beat 的英文 `visualKeywords`/`requiredVisual` 经 CLIP 文本编码，与片段代表帧的 CLIP 图像向量比余弦；与现有 bge/词面并存。模型缺失或向量未缓存时该分为 0，选镜不中断。

## 范围

- 新增 `src-tauri/src/storyboard/clip.rs`
- 改 `scoring.rs` / `phases.rs` / `storyboard.rs` / `segment_visual.rs` / `shot_replacement.rs` / `release_readiness.rs`
- `StoryboardSource.segment_clip_embedding`；`CandidateScore.clip`
- 资源：`clip-ViT-B-32-vision` / `clip-ViT-B-32-text` 配置进仓；ONNX 用 `scripts/fetch-clip-models.ps1`（gitignore，因 >100MB）
- 同步 `docs/api.md`、`docs/architecture.md`、`docs/decisions.md`、`docs/codebase/INTEGRATIONS.md`、`TASKS.md`

## 禁止变化

- 不替换 bge；不联网下载模型
- 不改 Phase 3/4 选镜契约；不改公开 Tauri 命令
- 不整库强制重跑视觉

## 契约

- CLIP 图像向量：`asset_segment_embeddings.model = Qdrant/clip-ViT-B-32-vision`，version=1，512 维，source_hash=帧内容 SHA-256
- 查询文本优先 keywords + requiredVisual（CLIP 偏英文）
- 发行就绪检查新增 `clip_model`（warn 级）
