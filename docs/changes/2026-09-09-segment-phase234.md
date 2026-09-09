# 片段级 Phase 2/3/4 选镜

## 结果

候选单位升级为素材片段：Phase 2 两级召回（素材 Top-20 → ensure → 片段 Top-12），Phase 3 选 `assetId+segmentId`，Phase 4 有片段时跳过 Pass A。

## 范围

- `StoryboardSource.segment` / `segmentEmbedding`；`StoryboardShot.segmentId`
- `storyboard_sources` 按需展开单段与双段候选
- `scoring` 用片段向量/时长/shotType
- Phase 2 重叠去重与两轮补位；Phase 3 picks 契约；Phase 4 真实片段窗口
- 同步 `docs/architecture.md`、`docs/api.md`、`docs/decisions.md`、`TASKS.md`

## 禁止变化

- 不改 `shot_replacement.rs`（待浅色工作区分支合入后再适配）
- 相邻镜头仍禁止同 `assetId`；40% 上限仍按素材计

## 契约

- Phase 3 响应优先 `picks:[{assetId,segmentId}]`，兼容旧 `assetIds`
- 未分段或 ensure 超时的素材仍以整条参与
