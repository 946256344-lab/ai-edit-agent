# 粗剪用上精修区间：镜头时长跟内容走、不复用、昼夜有序

## 现象

2026-09-27「Weekend Road Trip」（25 条 Pexels 公路旅行素材，16:9、BGM 开、配音关，要求 30 秒：上路 → 海边散步 → 弹吉他 → 夜里篝火收尾）：

- 12 镜全部 2500ms，节奏像节拍器。配音关时 brief 较长被判为 `full_script`，Phase 1 把简报标题拆成逐词「旁白」拍（Weekend / road / trip / recap.），而没有配音就没有时钟，镜头时长停在「目标时长 ÷ 拍数」。
- Phase 4 Pass B 提示词说「Rust 按端点计算 durationMs」，实际 `apply_shot_patches` 保留原时长，只换源区间。多镜源区间远长于槽位（3865–14208、683–12561、0–22568、4800–9600）：预览 `render_timeline_clip` 只播开头 2.5 秒，精修区间被丢；剪映 / CapCut（`clip_source_duration_us`）、FCPXML timeMap、OTIO 则按整段加速到 2.5 秒（最高约 9 倍速），预览与交付物不一致。
- 9100–10500（1.4 秒）被放慢到 2.5 秒。
- 同一素材在两拍各用一次（复用上限按 40% 算，12 镜允许 4 次）。
- 夜景车内镜头排在第 4 拍（7.5 秒），早于白天海滩段；Phase 1 还加了一段预告式标题蒙太奇，打乱简报顺序。

「部分完成」状态来自 agentloop 回执里的失败 / 待定工具，不在故事版结果路径，本次未改。

## 改动

- `media_options.voiceover == false` 时 Phase 1 锁 `key_message`；未给选项时仍按朗读时长判断。Phase 1 提示词要求保持简报事件顺序、不加预告蒙太奇，简报点明时段时写进 `requiredVisual` / `visualKeywords`。
- 新增 `SpeechTimingKind::Content`：无配音的整条生成用 `timing::content_plan`（节奏计划只作召回提示）。Phase 4 在 Content 下调用 `length::fit_shots_to_content`：每镜以精修区间长度为偏好，夹在 1.5–5 秒（`shotLengthHint` 快切 1–2.5 秒、长镜 2–8 秒；有屏幕标记的首镜不低于可读性下限），按各镜余量线性缩放到目标总长；窗口都够长但总长不够时放开 5 秒上限，窗口本身不够才按比例放慢。源区间与槽位等长：长了围绕视觉证据 `bestRange`（其次高光时刻）裁，短了在锁定窗内向两侧补，放慢时在 reason 写 `[slowed 0.xx: …]`。Pass B 提示词在 Content 下不再给拍时段，改为让模型按内容选 1.5–5 秒的区间。
- 任何时钟下 Phase 4 末尾都执行 `length::trim_sources_to_slots`，故事版不再留下「源区间长于槽位」。Voice 时钟（配音开）仍由 `fit_shots_scoped` 按口播定时长；局部编辑（Pacing）仍锁原槽位。
- 预览 `render_timeline_clip` 与交付链接器一致：整段源区间铺满槽位，长了加快、短了放慢，不再只播开头。
- Phase 3 进入选片前按 `preferred_asset_uses` 剔除已用素材：各拍候选池合起来的不同素材够每拍一条时每个素材只用一次，不够才平均摊开；40% 硬上限（`collect_phase3_issues` / `validate_storyboard`）不变，只在候选确实用尽时兜底。最终复用的镜头在 reason 写 `[reused asset: …]` 并记日志。
- 新增 `storyboard/daypart.rs`：Phase 2 按拍文字明说的时段（night / campfire / morning / 夜 / 白天 等）和证据 `timeOfDay` 过滤召回。第一个夜拍之前没说时段的拍不召回夜景，夜拍不召回白天片段，夜拍之间的拍不召回白天片段；`unknown` 与旧证据不参与，过滤后不足 9 段就不过滤。

## 已知限制

- `reselect_shots` 局部重选的复用过滤仍用 40% 上限，未改为「够就不复用」。
- 同素材区间交叠时的窗内挪切点（`resolve_overlaps_within_chosen_windows_scoped`）在窗口挤不下时仍可能缩短源区间，放慢不写 reason；复用收紧后极少出现。
- 时段只认明说的词，「dimly lit」这类措辞不算夜。
- 旧时间线里源区间长于槽位的镜头，预览会像剪映一样加速播放；需重新生成故事版才会按新规则裁区间。

## 同步文档

`docs/decisions.md`、`TASKS.md`。

## 验证

`cargo check` 通过；`cargo test --lib` 中 `storyboard`（161 条，含新增回归：内容时钟不截断不慢放、够素材不复用、夜拍前不召回夜景）、`preview`（27 条）、`handoff`（5 条）、`shot_replacement`（1 条）通过。待桌面实测：同一简报重新生成，检查镜头时长分布、源区间与槽位、复用与昼夜顺序。

## 决策

更新 `docs/decisions.md`：新增「没有配音时镜头时长跟画面内容走」，修订「源窗短于口播时放慢」的预览变速说明。
