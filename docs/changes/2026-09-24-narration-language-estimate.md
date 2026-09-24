# 2026-09-24：旁白时长按语言估算并收进成片目标

## 结果

TTS 逐拍时间戳对不上时，Phase 4 不再用「两字 300 毫秒」把中文口播估长。中文按字、英文按词、假名和谚文按音节计时，再按成片目标把各镜时长按权重摊开。画面总和回到口播目标，不再因为估算偏慢整轮打回精修。

## 范围

- `src-tauri/src/storyboard/phases.rs`：语言估算与托底分摊

## 禁止变化

- 不改公开 Tauri 命令
- 对得上 TTS 时间戳时仍走口播时钟，不走这条估算

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- narration_estimate_follows_language_and_target narration_duration_floor_extends_within_window`
