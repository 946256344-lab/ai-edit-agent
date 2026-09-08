# 2026-09-08: Phase 4 长窗 Pass C 收窄

## 问题

导入关键帧建的粗窗在长素材上常跨 15–20s，Pass B 每镜 6 帧时帧间距约 3s，切点精度约 ±1.5s。

## 改动

- `PHASE4_MAX_FRAME_SPACING_MS=1500`。
- Pass C：`uncertain` 仍整窗加密；非 uncertain 且 `window.span / 6 > 1500` 的镜围绕 Pass B 切点 ±max(500ms, 25% 跨度) 夹在窗内再抽 10 帧收窄；caption 标 `NARROW`。

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib storyboard::multimodal::`

## 同步文档

- `docs/architecture.md`
- `TASKS.md`
