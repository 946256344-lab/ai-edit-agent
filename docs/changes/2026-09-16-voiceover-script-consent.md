# 配音开着必须配音，没有稿先问同意

分支：`cursor/visual-batch-by-segment`。

## 结果

配音开关开着时，本轮必须合成旁白。用户已经给了可念稿，就照念去生成。只有主题、没有可念稿时，不在生成分镜里偷偷编稿；Agent 先写出旁白稿、问用户同意，同意后再把这篇稿当作 brief 生成并配音。

## 范围

- `src-tauri/src/storyboard.rs`：配音开着且 brief 还不是可念稿时直接拒绝生成
- `src-tauri/src/storyboard/phases.rs`：去掉「配音开着就在 Phase 1 编稿」
- `src-tauri/src/agentloop/native.rs`、`tools.rs`、`skills.rs`、`continuation.rs`：提示与失败码改为先问同意
- `docs/api.md`、`docs/architecture.md`、`docs/decisions.md`

## 禁止变化

- 不改公开 Tauri 命令签名
- 不改 40% 上限、第一次 6 段一批
- 配音关闭时仍不自动配音

## 验证

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- voiceover_script brief_has_voiceover storyboard::tests::system_locks_script_mode`
