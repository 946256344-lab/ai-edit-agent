# 关配音不再强制 15 秒

分支：`cursor/visual-batch-by-segment`。

## 结果

没有配音不再等于必须做成 ≤15 秒短片。`key_message` 仍是屏幕字、不配音；成片时长和 beat 数由模型按用户要求和内容决定，本地安全上限仍是 120 秒。用户没说时长时，提示建议 15–45 秒，每个镜头大约 2–3 秒（15 秒大约 5–7 镜），不是硬帽。标记可读性只跟所选目标时长比，不再夹 15 秒硬帽。

## 范围

- `src-tauri/src/storyboard.rs`：去掉短 brief 15 秒 / 5 beat 硬拒；标记可读性只用 `1.2×target`
- `src-tauri/src/storyboard/phases.rs`：Phase 1/3 建议每镜约 2–3 秒，15 秒大约 5–7 镜
- `src-tauri/src/agentloop/native.rs`：关配音建议 15–45 秒、每镜 2–3 秒；起草旁白建议 15–30 秒
- `docs/api.md`、`docs/architecture.md`、`docs/decisions.md`

## 禁止变化

- 不改公开 Tauri 命令、40% 上限、第一次 6 段一批
- 不新增长时滑轨
- 配音开着仍必须配音；120 秒仍是处理上限

## 验证

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- short_brief_ key_message_ locked_full_script`
