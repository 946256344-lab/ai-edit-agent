# Phase 2 跨语言召回与 Phase 3 信息补全

日期：2026-09-09  
分支：`fix/storyboard-asset-selection-scoring`

## 问题

本机就绪视频库抽样显示：视觉证据标签约 99% 为英文，beat 的 `requiredVisual`/`purpose` 约 90% 为中文。本地向量模型为 `bge-small-zh-v1.5`，跨语言余弦几乎无区分度；词面降级用中文双字匹配英文 blob，命中恒为 0。同时清晰度权重（原 0–25）与语义（原 0–30）接近，OCR 乱码进入向量文本，无证据素材与有证据素材混排。Phase 3 候选卡缺少 `requiredVisual`，模型无法按真实画面需求选片。

## 变更

- Phase 1：每个 beat 产出英文 `visualKeywords`（4–8 个具体名词/动作）。
- `semantic.rs`：`encode_beats` 查询加入 keywords；`ocr_is_meaningful` 过滤乱码；`EMBEDDING_VERSION` 升至 2（下次 storyboard 前本地重算向量，不重跑视觉模型）。
- `scoring.rs`：语义 0–50（余弦+词面各 25）、质量 0–10、时长 0–10、新鲜度 0–5；无证据素材排末层；返回分数分解与 `matchedKeywords`。
- Phase 2：候选池持久化 `scores`；相似去重忽略 OCR 乱码；日志输出前 8 名分数分解。
- Phase 3：池卡片补 `requiredVisual`/`visualKeywords`/`narration`/`onScreenText`；候选卡补 `retrievalScore`/`matchedKeywords`；网格上限 36→60。

## 验证

- `cargo fmt --check`、`cargo check`、`cargo test`（storyboard scoring/semantic/phases）
- `npm run lint`、`npm run build`
- 同 brief 重跑一次，日志中 Top-8 分数分解应让相关英文标签素材进入前列

## 同步文档

- `docs/api.md`
- `docs/architecture.md`
- `TASKS.md`
- `docs/changes/2026-09-09-phase2-cross-lingual-scoring.md`

## 后续（未做）

- 视觉分析 queued/failed 队列消化
- 「每 beat ≥2 镜」硬门是否放开
- 片段级检索 / 多语种向量模型
