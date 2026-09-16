# 2026-09-15: 素材切段只认已验证硬切

分支：`cursor/hard-cut-segments`。

## 结果

技术分析不再按秒均分素材。只保留 FFmpeg 检出、且 CLIP 确认两侧画面不像的硬切；CLIP 不可用、抽帧失败或切点两侧仍相似时整条一段。`analysisVersion=3`，旧就绪视频走既有空闲补跑，不自动排队视觉模型。

## 范围

- `src-tauri/src/assets/segments.rs`：删除均匀兜底与 24 段数量帽；CLIP 验切；长段抽帧上限 8
- `src-tauri/src/storyboard/clip.rs`：公开 `encode_image_bytes`
- `src-tauri/src/assets/analysis.rs`、`library.rs`：接入验真、待分段计数跟当前版本走
- 补跑时作废片段 CLIP 向量并把 `visualAnalysisVersion` 置 0，选镜按需再补
- 同步 `docs/architecture.md`、`docs/api.md`、`docs/decisions.md`、`TASKS.md`

## 禁止变化

- 不改 Phase 2/3/4 选镜与剪辑切法
- 不改两次视觉识别 JSON
- 不整库预跑视觉模型
- 不放开「每 beat ≥2 镜」硬门

## 契约

- `TechnicalMetadata.analysisVersion`：3=已验证硬切分段（2=旧均匀/未验真分段）
- 公开 Tauri 命令不变；`segmentPending` 统计低于当前分析版本的就绪视频
