# 2026-09-03: Storyboard 选片与精修分离 + 单步重试 + 可观测性

## 问题

旧流水线 Phase2「锁 1 主镜」与「每 beat ≥2 镜」冲突；Phase3 同时选片、拆镜、精修并扛终态校验；超时/schema 错误烧掉语义重试预算；接近成功的候选被整单丢掉；Native full-trace 不覆盖 Storyboard 直连；失败时 Agent 易换 brief 重开。

## 改动

- **修改1** `step_retry.rs`：传输预算与语义预算分离；校验失败带 `previousShots`；语义用尽后可 +1 校验尾修。
- **修改2** Phase2 仅本地 Top-12（去同/去相似后强制补位）；Phase3 `phase3_select` 从池选 2–3 互异 asset；Phase4 `phase4_refine_ranges` 只定时间段（禁止换片）；Phase5 `normalize`+`validate`，失败回 Phase4。
- **修改3** `qualityWarnings` / completion gaps 文案禁止为补 uncovered 重跑或改 brief；失败错误含 `partialCandidateSummary`；工具失败上下文要求同 brief 重试。
- **修改4** debug + `STORYBOARD_PROVIDER_TRACE=1` → `src-tauri/target/storyboard-provider-trace.jsonl`。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml storyboard::`（65 passed）
- `cargo test --manifest-path src-tauri/Cargo.toml storyboard_phase2_failure`
- `npm run harness:check`（提交前）
