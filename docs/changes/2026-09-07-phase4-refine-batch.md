# 2026-09-07: Phase 4 精修按镜拆批 + 每镜帧网格

## 问题

27 镜 Pass B 一次挂 162 张窗内帧时，自定义 API（agnes-2.5-flash）反复 `os error 10054` 断连；2 次传输重试后 Phase 4 失败。精修准确率要求不能靠减少每镜采样点数来瘦身。

## 改动

- 每镜仍抽满 `PHASE4_REFINE_FRAMES=6` / uncertain `PHASE4_UNCERTAIN_FRAMES=10`。
- 同一镜的定时帧拼成一张网格图（Pass B 3 列，Pass C 5 列），请求里每镜 1 张图。
- Pass B/C 按 `PHASE4_REFINE_SHOTS_PER_BATCH=10` 拆批；只合并本批 `orderIndex` 的源范围/`cropFocus` 等字段，拒绝换 `assetId`。
- 27 镜从「1 次 × 162 图」变为「3 次 × ≤10 网格图」，时间采样密度不变。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml compose_timed_frame_grid_keeps_all_cells`
- `cargo check --manifest-path src-tauri/Cargo.toml`

## 同步文档

- `docs/api.md`
- `docs/architecture.md`
- `TASKS.md`
