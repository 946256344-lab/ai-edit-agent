# 2026-09-04: Phase3/4 关键帧选片与「先选段再精修」

## 问题

Phase 3/4 纯文本猜切点；仅均匀/按时长加密抽帧，难避开准备段，也难防话说一半被切。随后 Phase 4 对每条锁定素材做全片场景切点扫描又过慢（常见 0–1 切点），并把多模态体量推高到易断连；`clamp_shots_to_chosen_windows` 在窗尾还会 panic。

## 改动

- Phase 3：候选附带 2×2 关键帧网格选片。
- Phase 4：
  1. **Pass A**：用**导入期关键帧时间**建内容候选窗（无关键帧则前/中/后三分段），每窗 1 张中点帧，模型只选 `windowId`（可标 `uncertain`）；**不再**对每条素材跑 FFmpeg 场景切点扫描
  2. **Pass B**：只在选中窗内抽 6 帧，精修 `sourceStart/End`
  3. **Pass C**：仅 `uncertain` 镜头在窗内再抽 10 帧复修
  4. Rust 把切点夹回选中窗（保证 `min ≤ max`，不再 panic）；旁白时长托底（画面不够念完则在窗内延尾）
- 取消按时长决定抽帧数。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml storyboard::multimodal::`
- `cargo test --manifest-path src-tauri/Cargo.toml storyboard::phases::tests::narration_duration_floor`
- `cargo test --manifest-path src-tauri/Cargo.toml storyboard::phases::tests::clamp_shots_survives`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `npm run harness:check`
