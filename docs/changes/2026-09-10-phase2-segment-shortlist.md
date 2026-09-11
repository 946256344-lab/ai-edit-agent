# Phase 2 锁片段短名单与 Phase 3 去似选镜

分支：`cursor/phase2-segment-shortlist`。

## 触发范围

- `src-tauri/src/storyboard.rs`：2a 短名单、片段展开、Phase 5 diversity
- `src-tauri/src/storyboard/phases.rs`：2b 池规则、相似硬门、Phase 3 校验与提示
- `src-tauri/src/shot_replacement.rs`：沿用新的 Phase 2 短名单签名

## 改动

- Phase 2a 每个 beat 召回 9 条互不相似的整片（整文件长得像的也互斥），并集做 `ensure_segment_visual`。有 `scene_segments` 的短名单素材一律展开为片段候选，不再因为片段视觉超时退回整条。
- Phase 2b 在这 9 条内保留约 4 段再补位到 Top-12，同片最多 2 段进入池；补不满不拉第 10 条相似片。后续 beat 的池避开与前面 beat 池视觉相似的段。Lead shot 带上真实 `segmentId` 和源范围。
- Phase 3 同一 beat 仍禁止同 `assetId`；跨 beat 允许同一素材的不同、不重叠、不相似片段，包括相邻镜。已用片段的相似画面硬拒（24×24 灰度均差 < 12，加上原有标签/向量）。池里仍有 ≥2 条可用互异素材时保持最少 2 镜，否则少镜或 uncovered，不拿相似段凑数。40% 素材占比保险丝保留。
- 本轮不引入 CLIP。

## 同步文档

`TASKS.md`、`docs/architecture.md`、`docs/api.md`、`docs/decisions.md`、`docs/changes/2026-09-10-phase2-segment-shortlist.md`。

## 验证

- `cargo fmt --check`、`cargo check --lib` 通过。
- `cargo test --lib storyboard::`：99 项通过，含新的 9 条去似短名单、同片最多 2 段、跨 beat 不同段允许相邻、相似硬拒回归。

## 决策

更新决策 11：有场景段时候选单位是片段；视觉未就绪不再等同整条素材。相邻同 `assetId` 禁令改为片段相似/交叠禁令。
