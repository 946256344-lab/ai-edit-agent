# 生成不等第一次分析，召回只要已打卡段

分支：`cursor/visual-batch-by-segment`。

## 结果

生成分镜不再为排队中的第一次视觉分析空等 65 秒。桌面入口仍可给 pending 批次提高优先级，但立刻用已经打上的段卡选片。没有第一次卡的段不进召回，也不再借用整条素材的标签充数。库里一张卡都没有时，直接报 `storyboard_visual_evidence_unavailable`。

## 范围

- `src-tauri/src/storyboard.rs`：去掉等待；展开时跳过未打卡段
- `src-tauri/src/assets/visual.rs`：删除 65 秒等待
- `docs/api.md`、`docs/architecture.md`、`docs/decisions.md`

## 禁止变化

- 不改公开 Tauri 命令
- 不改第一次 6 段一批
- 不整库预跑视觉模型

## 验证

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- storyboard::tests::storyboard_sources`
