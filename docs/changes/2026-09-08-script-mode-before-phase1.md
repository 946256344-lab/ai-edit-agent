# 2026-09-08: Phase 1 前由系统锁定 scriptMode

## 问题

模型常把可念稿标成 `key_message`，系统只能在 Phase 1 **之后**升格；判定顺序倒置，旁白/字幕链路容易半套 full_script、半套标记。

## 改动

- `decide_script_mode(brief)`：朗读估算 ≥约 20s → `full_script`，否则 `key_message`。
- `phase1_generate_narrative` 接收锁定模式，prompt 写明 REQUIRED，禁止改选。
- `enforce_decided_script_mode` 在响应后再次钉死模式并补齐 `spokenScript`。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml --lib system_locks_script_mode`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib speakable_brief`

## 同步文档

- `docs/api.md`、`docs/architecture.md`、`TASKS.md`
