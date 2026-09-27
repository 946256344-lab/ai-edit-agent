# 出预览后复查真实成片，能修的修一轮

## 现象

2026-09-27「Weekend Road Trip」12:22 会话 v2：素材 8973588 在第 2 镜（源 6000–7950）和第 10 镜（源 9000–14133）各用一次；第 4 镜 8976543（7.5 秒）是夜景，夹在白天的车内与海滩镜头之间，早于简报里排在前面的白天段。Agent 照常报告完成：此前没有任何一步看过渲染出来的成片，选镜各阶段只看候选素材和分析证据。

## 改动

- 新增 `src-tauri/src/agentloop/review_cut.rs`：
  - 从已渲染的预览按成片时间抽帧：每镜开头 / 中间 / 结尾 3 帧（短于 1.4 秒 2 帧、短于 0.7 秒 1 帧）；一次 FFmpeg 解码整条预览，数目对不上再逐帧补。
  - 拼接触表：一行一镜，格子按成片画幅（竖 180×320 / 横 320×180 / 方 240×240），每格左上角标 `#镜号 秒数s`。每个请求最多 4 张图、每张最多 8 行；超过 32 镜拆成并发请求（`post_model_payloads_concurrently`），只在 429 时退避。
  - 提示词附简报、拍序、每镜时间线事实与已有视觉证据（caption、scene、timeOfDay、setting、brightness、focus、源画面文字 / 标识 / 人群 / 展会），明确「帧上看到的为准」，字幕与标题是剪辑加的、不算问题。模型按 `repeated_footage` / `continuity` / `beat_order` / `weak_shot` / `text_logo_crowd` 逐镜给 high / medium / low 问题和修复建议（reselect / refine / reorder / none）。镜号不在本请求范围、类别未知的条目丢弃；重排只在一个请求看过全片且给出完整排列时接受。
  - 同一素材重复出现由时间线直接判定（`source: timeline`），不等模型：源区间重叠为 high，否则 medium；只重选后出现的那一拍。模型对同组镜头的重复报告并入，不重复计数。
  - 修复一轮，只修 high / medium：先 `refine_shot_ranges`（不改镜号，最多 10 镜），再 `reselect_shots`（最多 5 拍；不同替换描述分开调用、最多 3 次，避免一拍的描述混进另一拍的召回；同一拍一轮只重选一次），最后仅在时间线没有启用的配音 / 字幕轨且本轮没有重选时 `reorder_clips`。每步基于上一步落地的版本，失败只记原因、不回滚已落地的步骤。每条问题写 `repairStatus`：`applied` / `failed: …` / `not_attempted: …`。
  - 修复落地后重渲染预览并复核一次（只报告不再修）；复核仍有的问题进 `qualityWarnings`（`cut_review_<category>`，low 为 info）。本轮剩余不足 2 分钟或重渲染失败则不复核，未修的问题照常进警告，另加 info 级 `cut_review_unverified`。
- `skills.rs` `generate_storyboard`：原「渲染预览 + 交付编辑器」拆成两步，中间自动复查并修一轮；修复落地时结果的 `storyboardVersionId` / `versionNumber` / `timelineVersionId` / `previewTimelineVersionId` 指向修复后的版本，编辑器只交付该版本。结果新增 `cutReview`。本轮剩余不足 5 分钟或复查失败时 `cutReview.status` 为 `skipped` / `failed`，附 info 级 `cut_review_unavailable`，不挡生成。
- 不新增模型可调用工具：按 2026-09-27 产品方向（一次出基本满意的初剪，精修交给 CapCut 等编辑器），复查只在生成流程里自动运行。首版曾加过 `review_cut` 工具，已撤掉；Native 工具目录、fixture 与前端工具名不变。
- `native.rs`：系统提示要求按 `cutReview` 汇报，只有 `repairStatus=applied` 的修复才能说修好。
- `storyboard/multimodal.rs`：`fit_into_box`、`draw_cell_label` 改为 crate 内可见，字形表加 `#`。

## 已知限制

- 自动复查每次生成多一到两次模型请求，修复时再加局部重选 / 精修与一次重渲染，生成总时长会变长。
- 重排只接受模型给出的完整排列，且有配音或字幕时不自动做；这类问题只进警告。
- 超过 32 镜时各请求只看自己那段，跨段的近似画面只能靠时间线层面的「同素材」判定发现。
- 替换描述由复查模型写，会拼进该拍召回文本；提示词要求正面描述、不写否定。
- 手动插入的镜头只能报告，不能重选或精修。
- 用户想复查某个旧版本时没有入口；需要时重新生成。

## 同步文档

`docs/api.md`（`generate_storyboard` 结果字段）、`docs/decisions.md`、`TASKS.md`。

## 验证

`cargo check` 通过；`cargo test --lib agentloop` 通过，含新增 `reused_asset_is_flagged_and_only_the_later_shot_is_repicked`（本例回归）、`contact_sheets_never_exceed_the_image_limit_per_request`；`agent_contract_assets` 通过（工具目录未变）。只读核对本地数据库与预览（未启动应用）：12:22 会话 v2 的本地判定报出 8973588 第 2、10 镜重复（medium，计划只重选第 10 镜所在拍）；第 4 镜证据与真实预览帧均为夜景；按本模块同样的选择表达式从该预览抽帧 36/36。模型复查、自动修复与复核待桌面实测。
