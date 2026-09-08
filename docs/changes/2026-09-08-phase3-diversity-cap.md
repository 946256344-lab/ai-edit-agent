# 2026-09-08: Phase 3 补齐 40% 复用上限 + Phase 5 路由

## 问题

相邻同片已在 Phase 3 硬拒，但全序列 40% 复用上限只在 Phase 5 `validate_shot_diversity` 检查。小素材库下跨 beat 非相邻复用易在 Phase 3 通过、Phase 5 失败；`phase5_should_retry_phase4` 默认回 Phase 4，而 Phase 4 禁止换片，导致空转耗尽预算。

## 改动

- Phase 3 prompt 写出覆盖 beat 数下「全选 2 镜 / 全选 3 镜」时的 40% 数值上限，并提示池不足时优先 uncovered。
- `collect_phase3_issues` 增加 `asset_over_diversity_limit`（与 Phase 5 同一 `max_asset_uses_for_shot_count`）。
- `phase5_should_retry_phase4`：含 `diversity limit` / `reuse asset` 的错误不回 Phase 4。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml --lib phase3_rejects_asset_over_diversity_limit`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib phase5_does_not_retry_phase4_for_shot_cap_errors`

## 同步文档

- `docs/architecture.md`
- `TASKS.md`
