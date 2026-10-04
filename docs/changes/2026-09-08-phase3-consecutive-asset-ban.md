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

## 原长期文档补充：2026-09-08：Phase 3 相邻同片硬拒

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

~~最终播放序相邻镜头不得共用同一 `assetId`（含跨 beat）。~~ 已被 2026-09-10 片段相似/交叠硬拒取代；非相邻复用仍受 40% 上限。见 `docs/changes/2026-09-08-phase3-consecutive-asset-ban.md`。
