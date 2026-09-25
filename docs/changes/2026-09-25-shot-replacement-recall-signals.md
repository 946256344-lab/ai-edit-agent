# 2026-09-25：替换镜头推荐与生成分镜用同一套召回信号

## 结果

替换镜头面板点「重新推荐」时，`generate_shot_recommendations` 重跑 Phase 2 不再只靠词面和质量分排序，而是和生成分镜一样带上：

- 本地语义向量（`semantic::encode_beats`）和 CLIP 图文向量（`clip::encode_beats`），两路并行编码；任一路失败 `log::warn` 后降级为空向量，不挡推荐
- 项目内各剪辑会话最新时间线的素材使用次数（复用 `storyboard_usage_counts`），与主链路同样扣分
- 每拍目标时长取当前时间线里该拍镜头的实际总时长；时间线里没有镜头的拍回落均分
- 候选池出来后为池内素材补 CLIP 图像向量（`clip::refresh_assets_clip_embeddings`），失败只记日志

## 范围

- `src-tauri/src/shot_replacement.rs`：只改 `generate_shot_recommendations`
- `src-tauri/src/storyboard.rs`：`storyboard_usage_counts` 改为 `pub(crate)`，逻辑不变

## 禁止变化

- 不改公开 Tauri 命令签名、候选池存储格式、`prepare_shot_replacement` 与 `recommendations`
- 不改 Phase 2 打分、去重与扩池规则

## 边界

- 该命令不在 Agent 执行截止时间下运行，编码线程不设截止作用域
- 不在推荐前补跑片段视觉证据（主链路的 `ensure_segment_visual_evidence`），仍只用已就绪片段

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml`：通过，本次改动无新增警告
- 真实桌面替换面板排序效果待验收
