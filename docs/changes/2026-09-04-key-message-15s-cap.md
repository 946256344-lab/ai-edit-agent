# 2026-09-04: key_message 收敛为 ≤15s 短视频

## 问题

`key_message` 原先按 15–45s、3–8 beat 引导，短目标 brief 仍容易做出偏长成片。

## 改动

- `SHORT_BRIEF_TARGET_CAP_MS`：45s → **15s**。
- Phase1 prompt：`key_message` 默认 **8–15s**、约 **2–5 beat**；短 brief 禁止再扩成 30–90s。
- `short_brief_duration_issue`：超时 / 超 beat 文案与上限对齐。
- `docs/api.md`、`docs/architecture.md` 同步。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml short_brief_rejects_inflated_full_script_duration -- --nocapture`
- `npm run harness:check`
