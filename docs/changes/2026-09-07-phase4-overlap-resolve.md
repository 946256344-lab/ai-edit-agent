# 2026-09-07: Phase 4/normalize 窗内机械消交叠

## 问题

Pass B 两镜同素材同切点时，带 `cropFocus` 会跳过 normalize 消交叠，Phase 5 报源范围交叠后整轮重跑 Phase 4。

## 改动

- Phase 4 `clamp` 后在已选内容窗内消交叠；满窗争用时窗内均分，不越窗。
- normalize 始终消交叠；有构图的素材不做整段 pack；被挪源范围的镜头清掉 `cropFocus`。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml resolve_overlaps_packs_identical_cuts_inside_shared_window`
- `cargo test --manifest-path src-tauri/Cargo.toml normalize_clears_crop_focus_when_overlap_resolve_moves_range`

## 同步文档

- `docs/api.md`
- `docs/architecture.md`
- `TASKS.md`
