# Phase 1 按库存写 beat

## 结果

Phase 1 在写叙事前注入本地库视觉/OCR 库存摘要（高频 tags/scenes/OCR），约束 `requiredVisual` / `visualKeywords` 贴近库里真实有的画面；brief 叙事意图保留，禁止编造库存没有的主体。

## 范围

- `phases.rs`：`build_library_inventory_summary` + `phase1_generate_narrative` 增加 inventory 参数
- `storyboard.rs`：加载 sources 后生成摘要并传入 Phase 1
- 同步 `TASKS.md`、`docs/decisions.md`、`docs/api.md`、`docs/architecture.md`

## 禁止变化

- 不改 Phase 2–5 选镜契约
- 不把素材 ID/路径/原图发给模型
- 不做全库视觉重跑

## 契约

- 库存摘要上限约 3500 字符；无证据时给保守提示
- Phase 1 仍不选片；缺口靠后续 uncovered / 少镜，不靠假视觉词凑满
