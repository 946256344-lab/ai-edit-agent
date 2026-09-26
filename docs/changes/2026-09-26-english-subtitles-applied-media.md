# 英文对齐字幕保留词间空格，Agent 按实际落地媒体汇报

## 现象

审片「测试1」最新预览（英文旁白，Fish Audio 配音）：

- 字幕单词之间没有空格，例如 `precisionmeetspoweronthe`。
- 每 24 个字符硬断行，出现 `pinpo / int`、`manufacturi / ng`，默认字号下超出 540 宽竖屏两侧。
- `search_music` 失败（Jamendo 未配置），时间线 `musicTracks` 为空，Agent 仍回复「Music: Background track added」。

## 根因

- Fish 返回逐词 `segments`，文本不带空格；`group_alignment_units` 按中文逐字习惯直接拼接。
- `wrap_subtitle_text` 按字符数截断，不看单词边界。
- `generate_storyboard` 工具输出只回显 `requestedMedia`（`bgm: true`），没有给出时间线上实际存在的轨道，模型把「要求」当成「已完成」。

## 触发范围

- `src-tauri/src/voice_provider.rs`：逐词 alignment 在两侧均为非中日文字时补空格；拉丁文字只在空格处换行，每行最多 20 字符、每条最多两行（40 字符），两行时取最接近中点的空格；中文仍每行 8 字、每条 16 字。
- `src-tauri/src/agentloop/skills.rs`：`generate_storyboard` 输出新增 `appliedMedia`（按时间线启用且非空的配音、字幕、音乐轨）、`timelineVersionNumber`，要求了却没落地的媒体列在 `mediaNotApplied` 并给恢复方式；工具消息同步写明。
- `src-tauri/src/agentloop/native.rs`：系统提示要求配音、字幕、音乐只按 `appliedMedia` 汇报，`mediaNotApplied` 必须如实告诉用户；不得声称的产物清单加入字幕和音乐。

## 改动

公开 Tauri 命令不变。Native 工具 `generate_storyboard` 输出新增字段，无持久化变化。已生成的旧时间线字幕不会自动修正，需要重新生成配音或故事版。

## 同步文档

`docs/api.md`、`TASKS.md`。

## 验证

`cargo check` 通过；`cargo test --lib` 过滤 alignment / subtitle / voice / generate_storyboard 全部通过，新增回归 `fish_word_alignment_keeps_spaces_and_wraps_at_words`。待桌面用英文旁白重跑一条，确认字幕、剪映草稿与 Agent 回复。

## 决策

无。
