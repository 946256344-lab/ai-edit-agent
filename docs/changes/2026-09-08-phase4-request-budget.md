# 2026-09-08: Phase 4 Pass A 拆批与 Phase 3 关键帧公平分配

## 问题

Pass B 已按镜拆批，但 Pass A 仍把全部素材的全部窗中点帧塞进一次请求，多 beat 选片后易到 80–100 图断连。Phase 3 关键帧预算按池顺序分配，后排 beat 易饿死。导入关键帧在 1s 处切出 [0,1s) 陷阱头窗。

## 改动

- `PHASE4_PASS_A_MAX_IMAGES=40`；Pass A 按素材贪心拆批，每批只带本批 shot/window 与帧。
- `phase4_content_windows`：首窗 &lt;1.2s 且存在后窗时向前并入。
- Phase 3 关键帧按名次跨池轮询至 36；候选卡增加 `keyframeGridAttached`。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml --lib content_windows_merge_short_head`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib phase3_pool_cards_expose_full_shortlist`

## 同步文档

- `docs/architecture.md`
- `TASKS.md`
