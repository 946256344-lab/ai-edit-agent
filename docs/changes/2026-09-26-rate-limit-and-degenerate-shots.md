# 限流不再做出缺拍无声成片，窗外入出点不再变成 1ms 定格

## 现象

切到 `agnes-3.0-flash` 后审片「测试1」（时间线 v1，9 镜）：

- 无配音轨；字幕是被截成 40 字符的镜头文字（`without paus`、`work in t`）。
- 第 5、6、8、9 镜源范围只有 1ms（如 8332–8333），放慢到 2–3 秒成定格。
- 预览只有 14.8 秒（时间线 23.8 秒）、无音轨，仍标记 `preview_ready`。
- 两拍 uncovered；Agent 补镜时再遇 429 中断。
- 浅色画面上的白字字幕看不清。

## 根因

- key 仍是免费档（每分钟 10 次），Phase 3 三拍收到 429。`is_final_model_failure` 不把 429 视为终态，Phase 3 在约 2 秒内打完 3 次重试后把该拍留空；画面短于配音，自动配音跳过，字幕退回 `subtitle_text_from_narration` 的 40 字符硬截断。
- Phase 4 返回的区间大半落在锁定窗外，`clamp_shots_to_chosen_windows_scoped` 把起点夹到窗尾减 1ms，得到 1ms 源窗；预览放慢倍数无上限，这几镜几乎不出帧，拼出的预览变短但没有核对时长。
- `write_text_tracks_ass` 的 Style 行比 Format 行错一列：`BorderStyle` 收到对齐值，`Outline`/`Shadow` 恒为 0，描边与阴影值落进边距；默认字幕样式本身也没有描边。

## 触发范围

- `src-tauri/src/provider.rs`：自定义 API 与网关遇 429 按 `Retry-After`（缺省 2/4/8/16 秒，单次最多 30 秒）重试最多 4 次，不超过执行截止时间；之后仍 429 视为终态失败。
- `src-tauri/src/storyboard/phases.rs`：模型区间与锁定窗重叠不足一半时，改取窗中央、按口播时长的一段并记 warn（含模型原区间）。
- `src-tauri/src/preview.rs`：预览成片比时间线短 500ms 以上即报错，不标记 `preview_ready`；ASS Style 列序与 Format 对齐。
- `src-tauri/src/models.rs`：`TextStyle` 默认改为白字黑描边 6.0 + 阴影（同字幕预设 `classic_stroke`），配音对齐字幕与镜头文字字幕均使用；剪映/CapCut 草稿按既有映射得到边框宽度 60。
- `src-tauri/src/storyboard.rs`：无配音时字幕兜底中文仍 40 字，拉丁文字保留整句（最多 80 字符、只在空格处截断）。
- `src-tauri/src/storyboard/provider_trace.rs`：debug 下 Phase 4 返回默认写入 `storyboard-pool-trace.jsonl`。

## 改动

公开命令与 schema 不变。新时间线的字幕默认带描边；已有时间线的样式不变。限流时生成如实失败，不再产出缺拍成片。

## 同步文档

`docs/api.md`（模型重试规则）、`TASKS.md`。

## 验证

`cargo test --lib` 436 条通过（`tool_execution_receives_and_obeys_the_run_deadline` 只给 100ms 预算，机器忙时偶发失败，改动前同样如此），新增回归：`rate_limit_waits_before_retrying_within_the_deadline`、`english_subtitle_fallback_keeps_whole_words`、`preview_shorter_than_timeline_is_a_failure`，并扩展 `clamp_shots_survives_when_start_already_at_window_end` 断言不再得到 1ms。待换 Token Plan key 后桌面重跑一条确认。

## 决策

无。
