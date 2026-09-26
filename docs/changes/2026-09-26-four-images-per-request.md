# 每个模型请求最多 4 张图，多出的拼图或拆成并发请求

## 现象

换成 Agnes Token Plan key 后，生成在 Phase 3 第一拍即失败：`HTTP 400`，Agent 回复「生成工具失败、没有产物」。开发版补记被拒原因后，上游返回：

```
Image count 9 exceeds limit 4 per request.
```

免费档没有此限制，所以此前同一模型下 Phase 3 能通过。

## 根因

多处请求一次带超过 4 张图：Phase 3 每拍 9 张候选网格（弱匹配扩到 12）；Phase 4 Pass B/C 每批最多 10 镜、每镜一张网格；粗视觉每批 6 张；片段加深每批 12 帧。

## 触发范围

- `src-tauri/src/provider.rs`：`MAX_IMAGES_PER_REQUEST = 4`；发送前统计请求里的图片块，超出即返回 `provider_request_too_many_images`（终态，不重试）。新增 `post_model_payloads_concurrently`，同一步内彼此独立的请求同时发出、按顺序返回，工作线程继承截止时间。
- `src-tauri/src/storyboard/phases.rs` / `multimodal.rs`：Phase 3 候选网格按段上下拼接（9 条 → 3 张、12 条 → 4 张，每段宽 640、灰色分隔条），文字写明每段对应的 `candidateIndex`；拼接图按网格路径缓存在 `derived/phase3_stacks`。
- `src-tauri/src/storyboard/phase4.rs` / `multimodal.rs`：Pass B/C 每批最多 4 镜，各批先按顺序准备（本地抽帧与缓存），再并发请求，再按原顺序合并。各批看到的草稿是本轮开始时的版本（此前串行时后一批能看到前一批结果）；各批只改自己的镜头，同素材交叠仍由窗内机械消交叠处理。Pass A 每批上限同为 4。
- `src-tauri/src/assets/visual.rs`：粗视觉任务大小不变（最多 6 段，旧排队任务仍有效）；执行时按素材分组发请求（每个请求一条素材、最多 4 张），各组并发，只采用本组素材的结果。不同素材不再同请求，根治描述挂错素材。
- `src-tauri/src/assets/segment_visual.rs`：片段加深同样按素材分组、每请求最多 4 帧、并发，只采用本组素材结果。

## 改动

公开命令与 schema 不变。请求数增加、并发发送（用户已确认只按需要设计、不考虑成本）。已存在的串位描述不会自动修正，另行用本机 CLIP 离线修复。

## 同步文档

`docs/decisions.md`、`docs/api.md`、`docs/architecture.md`、`TASKS.md`。

## 验证

`cargo test --lib` 438 条通过；新增回归 `requests_over_the_image_limit_are_counted_and_final`、`visual_requests_never_mix_assets`，扩展 `compose_timed_frame_grid_keeps_all_cells` 覆盖拼接尺寸；Phase 4 批次测试改按批大小常量构造。待桌面用 Token Plan key 重跑确认。

## 决策

每个模型请求最多 4 张图、画面分析按素材分组并发，已写入 `docs/decisions.md`。
