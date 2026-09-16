# 粗视觉按硬切段组批

分支：`cursor/visual-batch-by-segment`。

## 结果

第一次视觉识别按硬切段各送中点 1 帧，最多 6 段一批。模型用自己的话说这段镜头能干什么（`narrativeRole`），并写一句可见内容（`caption`）；不给选项、不校验枚举。身份对不上的单卡丢掉，不因一张废整批。素材全部段有卡后粗识别 `ready`，不抬 `visualAnalysisVersion`。选中镜头的多帧精修见 `docs/changes/2026-09-16-visual-second-is-refine.md`。

## 范围

- `src-tauri/src/assets/visual.rs`：按段组批、自由叙事 prompt、按段回写、单卡身份绑定
- `src-tauri/src/assets/segment_visual.rs`：已有粗卡不跳过、不把粗卡当成 version=2、保留 narrativeRole/caption
- `src-tauri/src/models.rs`：`VisualEvidence.narrativeRole` / `caption`
- `src/lib/local-store.ts`、`src/components/asset-workspace/AssetEvidenceInspector.tsx`：展示叙事短语
- `docs/api.md`、`docs/architecture.md`、`docs/decisions.md`

## 禁止变化

- 不把 `narrativeRole` 收成封闭枚举
- 不整库预跑第二次片段视觉
- 不改公开 Tauri 命令、超时或每批 6 张
- 不抬 `visualAnalysisVersion`

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml --lib -- assets::visual`
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
