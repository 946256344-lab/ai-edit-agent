# 片段级技术分析与版本化补跑

## 结果

技术分析改为真实场景分段（FFmpeg 低分辨率检测 + 均匀兜底），`analysis_version=2`；旧就绪视频在队列空闲时后台补跑，不触发视觉请求。证据面板按片段展示，素材页显示待分段计数。

## 范围

- 新增 `src-tauri/src/assets/segments.rs`
- 改 `src-tauri/src/assets/analysis.rs`（接入分段、`reanalyze_asset_segments` 补跑）
- 改 `src-tauri/src/models.rs`（`analysis_version`、`SceneSegment` 扩展、`AssetEvidence.segments`）
- 改 `src-tauri/src/assets/library.rs`、前端证据面板与素材计数
- 同步 `docs/api.md`、`docs/architecture.md`、`TASKS.md`

## 禁止变化

- 不改 Phase 2/3/4 选镜逻辑（后续 PR）
- 不触发整库视觉模型补跑
- 不覆盖已有 `visual_evidence`

## 契约

- `get_asset_evidence` 增 `analysisVersion`、`segments[]`
- `AssetPage.counts` 增 `segmentPending`
- `TechnicalMetadata.analysisVersion`：0=旧固定帧，2=真实分段
