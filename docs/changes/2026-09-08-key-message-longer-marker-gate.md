# 2026-09-08: 更长成片请求仍校验 key_message 标记

## 问题

`key_message_marker_issue` 在 `brief_requests_longer_runtime` 时整段跳过，空标记和超 24 字都放行；15s 帽放宽逻辑成了死代码。

## 改动

- 仅大段可朗读文案跳过标记门禁（仍交给 full_script 纠偏）。
- 用户要求更长时继续校验空标记 / ≤24 字，时长上限改为 `1.2×target`，不再夹 15s。

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml --lib longer_runtime_brief_still_requires_key_message_markers`

## 同步文档

- `docs/api.md`、`TASKS.md`
