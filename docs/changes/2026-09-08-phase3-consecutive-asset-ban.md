# 2026-09-08: Phase 3 相邻同片硬拒

## 问题

相邻镜头允许同 `assetId`（仅交叠时 Phase 5 才拒），跨 beat 衔接易在 Phase 4 切出交叠源窗并整轮重跑精修。

## 改动

- Phase 3 prompt + `collect_phase3_issues`：最终播放序相邻镜不得同 `assetId`（含跨 beat），issue kind=`consecutive_duplicate_asset`。
- Phase 5 `validate_shot_diversity` 与之对齐：相邻同片一律拒绝；非相邻复用仍受 40% 上限，交叠仍由源范围校验处理。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml --lib phase3_rejects_consecutive_same_asset_across_beats`

## 同步文档

- `docs/api.md`
- `docs/architecture.md`
- `TASKS.md`
