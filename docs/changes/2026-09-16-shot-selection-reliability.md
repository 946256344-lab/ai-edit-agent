# 选镜头：配音短 brief 与旧整片卡

分支：`cursor/visual-batch-by-segment`。

## 结果

配音开启、系统已把短 brief 锁成 `full_script` 时，Phase 1 不再用「必须改成 key_message」打回；15 秒时长帽仍在。有硬切段但段上还没有第一次卡、素材级却有旧整片卡时，按整条进召回，不把整片标签抄到未打卡段上。段上已有卡时仍只收打卡段。

## 范围

- `src-tauri/src/storyboard.rs`：短 brief 校验看系统锁定的 scriptMode；展开为空则回退整片卡
- `docs/api.md`、`docs/architecture.md`、`docs/decisions.md`

## 禁止变化

- 不改公开 Tauri 命令、40% 上限、第一次 6 段一批
- 不把未打卡段标成已识别
- 不改相似度硬拒

## 验证

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- storyboard::tests`
