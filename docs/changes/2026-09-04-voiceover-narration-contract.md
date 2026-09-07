# 2026-09-04: 旁白契约 — 可念稿 full_script、去重 join、key_message 硬门

## 问题

完整英文可念稿被标成 `key_message`（~12s）；normalize 把 beat 旁白复制到同 beat 每镜；自动配音 join 全镜 → 朗读约 2.4×；画面短于语音后 fit 失败，预览无声且提示像「未配置」。

## 改动

- Phase1 后：`brief_has_substantial_speakable_copy` 时强制 `full_script`（必要时抬 `targetDurationMs`），走已有 audio-first。
- 纠偏在 Phase1 **验收前**执行；可念稿跳过 `key_message` 旁白硬门，避免误拒后耗尽重试。
- `key_message`（无大段可念稿）：beats 旁白估计时长超过 `min(target×1.2, 15s)` 则 Phase1 重试。
- normalize：仅 lead 回填 `narrationText`；bridge/tail 空旁白，字幕仍可从 beat 节选。
- `storyboard_narration_text`：优先 `beats[].narration`；否则 lead 去重。
- 自动配音 `voiceover_longer_than_picture`：写入 `qualityWarnings` + 明确文案；**不**因该警告推迟预览。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml key_message_rejects_narration`
- `cargo test --manifest-path src-tauri/Cargo.toml normalize_only_fills_lead`
- `cargo test --manifest-path src-tauri/Cargo.toml storyboard_narration_prefers_beats`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `npm run harness:check`

## 同步文档

- `docs/api.md`
- `docs/architecture.md`
- `TASKS.md`
