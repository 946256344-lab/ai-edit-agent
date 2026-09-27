# 画幅选择与 BGM 自动配乐

## 现象

- 输入框下方只有「配音 · 字幕 · BGM」三个开关，成片只能是竖屏：预览、剪映 / CapCut 草稿画布写死 540×960，用户在对话里写「16:9」也无效。
- BGM 开着也经常没有音乐：`generate_storyboard` 从不加音乐，要靠模型自己再调 `search_music` / `use_online_music`；Jamendo 未配置（官方公共测试 ID 已停用）时这条路必然失败，素材库里用户导入的音频也不会被用上。

## 改动

- 输入框按钮改为「画幅 · BGM · 配音」。字幕不再单独开关，随配音发送（`subtitles = voiceover`）。画幅是下拉选择（9:16 竖屏 / 16:9 横屏 / 1:1 方形），默认 9:16。
- `MediaOptions` 增加可选 `aspectRatio`（`"9:16" | "16:9" | "1:1"`），缺省为 9:16，随现有 `content_json.mediaOptions` 快照写入分镜；无新增表或列，旧分镜按竖屏。模型传 `mediaOptions` 却漏了 `aspectRatio` 时沿用输入框选择，不静默退回竖屏；用户本轮文字点名其他比例时按文字。
- 画布按时间线所属分镜的画幅：9:16 为 540×960，16:9 为 960×540，1:1 为 720×720。本地预览（含试选镜头）、ASS 字幕坐标与字号（按画布长边换算，横竖屏字号一致）、画中画尺寸（画布三分之一）、剪映 / CapCut 草稿画布与主体裁切、FCPXML 画布都用同一画布。预览片段缓存键对 9:16 保持不变，其他画幅单独成键。
- Phase 4 精修 `cropFocus` 时按所选画幅描述裁切框。
- BGM 开启且时间线还没有音乐时，`generate_storyboard` 在配音之后直接写一条音乐轨：先从素材库里用户导入、已分析、未排除、源文件可访问的音频中挑（文件名命中简报情绪词优先，其次能覆盖全片，再其次更长；应用自己生成的配音和 Jamendo 下载不算）；没有再按简报情绪标签找 Jamendo CC0 / CC-BY 器乐曲（`fuzzytags` + `vocalinstrumental=instrumental`），下载分析后写入并保留署名。两条路都不可用时 `mediaNotApplied.bgm` 写明两边的原因和恢复方式，Agent 必须如实转告。音量沿用有旁白 0.15、无旁白 0.35，结尾淡出 1.2 秒。

## 已知限制

- Phase 3 候选卡的 `verticalCropFit` 仍按 9:16 描述；横屏成片时这条标签可能让模型略微偏好适合竖裁的镜头。裁切本身由 Phase 4 按所选画幅决定。
- 字幕每行字数仍按竖屏（拉丁 20 字符、中文 8 字）换行；横屏与方形画布更宽，不会越界，只是行偏短。
- 预览面板的镜头缩略图仍按 9:16 裁切显示。

## 同步文档

`docs/api.md`、`docs/decisions.md`、`TASKS.md`。

## 验证

`cargo check`、`cargo check --tests` 通过；`cargo test --lib` 中 `media_options`、`auto_music`、`handoff`（5 条）、`preview`（27 条）、`agentloop`（115 条）通过；剪映适配器 Python 测试 21 条通过；`npm run lint`、`tsc -b` 通过。待桌面实测：三种画幅的预览与 CapCut 草稿、素材库有音频 / 仅 Jamendo / 两者都无时的配乐与提示。
