# Storyboard 候选只允许技术就绪视频

日期：2026-08-26
分支：`codex/video-only-storyboard-candidates`

## 目标

保证每个 beat 的 Top 5 来自真正可执行的视频镜头池：素材必须属于当前项目、技术分析状态为 `ready`、`kind = video`、未被用户排除且源文件当前可访问。候选不足 5 个时返回实际数量，不使用图片、音频或其他类型补足。

## 根因

原查询只限制 `analysis_status = 'ready'`，会把可访问的图片和音频也交给评分器。评分器又把所有非视频素材按“图片时长灵活”加满时长分，导致音频可能进入甚至排在逐 beat Top 5 首位。模型选中音频后，最终 storyboard 校验才拒绝它，造成同一结构性错误在 Phase 3 重试三次。

## 变更

- `storyboard_sources` 在候选事实入口增加 `kind = 'video'` 硬过滤，保留项目、技术状态、用户排除和文件可访问性边界。
- Phase 2 与旧候选提示明确所有候选均为视频，源时间必须位于已验证时长内。
- 新增 SQLite 回归，覆盖 ready 视频、非 ready 视频、图片、音频、用户排除和文件缺失六种情况。

## 当前优先选择机制

选镜分两层：

1. Rust 预排序：以 beat 的 `requiredVisual + purpose` 生成词元；英文使用连续字母数字词元，中文使用相邻双字，检查其是否出现在视觉证据的 subjects/actions/products/scene 或 OCR 中，形成 0–50 的语义分。再叠加画面质量、时长匹配、连续复用惩罚和新鲜度分。当前独立质量值缺失时统一按 0.5，新鲜度仍固定为 10，因此主要有效差异来自词面命中、时长和连续复用。
2. 模型复选：每个 beat 最多收到 5 个候选的 asset ID、时长、场景段、最多 12 个视觉标签和可读取的关键帧网格；模型直接查看这些证据后返回 `assetId`、源时间范围、理由与 `matchLevel`，或返回 uncovered。

因此 Rust 预排序目前很大程度是关键词/字面重合，不是 embedding；最终选择是模型基于候选证据和关键帧的多模态判断。

## 不变边界

- 不改变公开 Tauri 命令、Provider、SQLite schema 或最终 storyboard 类型。
- Rust 继续校验项目作用域、素材资格、源时间范围和真实版本写入；模型只在合格视频候选中做创作判断。
- 图片仍可保留在素材库和既有历史产物中，但不进入新的 storyboard Top 5。

## 文档同步

- `docs/architecture.md`
- `docs/api.md`
- `TASKS.md`

## 验证

- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `cargo test --manifest-path src-tauri/Cargo.toml`：233 个库测试、2 个契约测试通过
- `python -m unittest discover -s src-tauri/scripts -p "test_*.py"`：14 个测试通过
- `npm run agent:check`
- `npm run branch:check`
- `npm run harness:test`
- `npm run harness:check`
- 独立审查：无 P0-P2 可验证问题
