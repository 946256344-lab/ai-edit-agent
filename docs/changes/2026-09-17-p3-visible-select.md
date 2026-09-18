# Phase 3 按可见证据选片，网格按候选时间窗现拼

## 结果

Phase 3 候选卡带回画面描述句（caption/scene/subjects/actions）。选片先对 `requiredVisual` 与图/描述，旁白只约束时长和口播，不单独决定选哪条；对不上就诚实情景承载，不按题材禁选。每一条候选的网格按该条时间窗的段帧拼；文件缺失则从源片现抽，不再只读可能过期的整片旧网格。无图仍可凭描述选，不作为硬失败。`matchLevel` 由模型标 `direct` 或 `contextual`。

## 范围

- `src-tauri/src/storyboard/phases.rs`
- `src-tauri/src/storyboard/multimodal.rs`
- `src-tauri/src/storyboard.rs`

## 禁止变化

- 不改公开 Tauri 命令
- 不改 0–4 序号、40% 上限、相似硬拒
- 不强制直证、不强制有图才合法、不按画面类型黑名单
