# 文本轨基础模型

## 范围

为内部时间线增加版本化 `textTracks` 内容槽位，以及文本轨、cue、样式、布局和受限动画的数据模型。旧 `content_json` 不含该字段时安全读取为空数组；既有视频编辑和 preview 更新会保留文本轨。

## 交付边界

本次开放 `replace_text_tracks`：模型可提交完整文本轨，由 Rust 校验 cue 时间、布局/样式范围、受限基础动态和唯一 ID，再创建带完整 before/after 审计的新时间线版本。后端会把所有 cue 的剪映兼容性固定为 `local_preview_only`，不接受模型自证。已实现本地 preview：已启用文本轨会编译为 ASS 后经 FFmpeg/libass 叠加，且通过实际 FFmpeg 回归用例。Jianying 文本草稿映射仍未开放；虽然当前开发机的 `pyJianYingDraft` 可发现文本 API，该能力尚未在 Jianying Pro 8.0 中完成真实草稿验证，不能表述为支持或交付。

本机已创建并注册包含静态、淡入和向上滑入文本的唯一 Jianying 测试草稿；其 `draft_content.json` 结构包含视频轨、文本轨、三个文本 segment 和动画引用。用户已在 Jianying Pro 8.0 中确认画面正常，因此后端把 `jianying_default` 字体、无描边/阴影/背景、无出场或循环的静态/淡入/向上滑入 cue 标记为 `verified` 并写入新草稿；其余 cue 仍会被拒绝，避免静默省略未认证文本。

模型可通过新增的只读 `get_text_capabilities` 技能观察字体和效果模板目录。目录将 local preview 能力与已验证 Jianying 交付能力分开，模型保留创作选择权，而后端保留兼容性状态的权威性。

前端时间线投影与 Agent 对话工作区增加文本 cue 审阅：展示时间、字体、入场模板与兼容状态，但不读取或同步其他 Jianying 草稿。

## 同步文档

- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`

## 后续扩展

适配器已增加 `TextBorder`、`TextBackground`、`TextShadow` 与剪映内置字体资源的写入路径，并把文本样式的颜色、描边、对齐、间距和安全区收紧为受限输入。它们在 Jianying Pro 8.0 完成逐项视觉验收前，仍由运行时标记为 `local_preview_only`，不能越过交付门。

文本能力目录新增由后端解析的文本预设：`subtitle_safe` 和 `headline_rise` 是已验证的可交付配方，`callout_card` 与 `cta_card` 只用于 local preview。模型只需选择合适的预设和内容；Rust 会以确定的样式、布局和动态替代冲突参数，并在版本化时间线与审计记录中保留实际结果。

`TextCue` 对 Agent 输入采用安全默认样式与布局；因此模型可以只发送 cue 的 ID、时间和内容，或再选择一个预设，而无需伪造 `jianyingCompatibility` 或填写将被预设覆盖的参数。

用户已在 Jianying Pro 8.0 中确认第二轮测试草稿的淡出、弹入和向下滑入显示正常。已验证交付矩阵据此扩展为：`jianying_default` 下的静态、淡入/淡出、向上滑入、向下滑入和弹入；新增 `headline_pop` 作为已验证预设。描边、背景、阴影和内置字体仍未进入交付矩阵。

`TextTrack.layer` 已贯通 local preview 与 Jianying adapter：ASS 使用 event layer，Jianying 为每个已启用文本轨创建独立命名的 text track 并按 layer 排序。后端拒绝同一文本轨内的时间重叠 cue，允许跨 layer 的受控叠放。

## 后续验证（2026-08-11）

- 在当前 Jianying Pro 11.2 实机验证：将每条文本素材内嵌的 `content` JSON 以 Unicode 转义写入后，中文“中文字幕”可在草稿画面与文本轨中正确显示。
- 已验证交付预设增加 `headline_drop`；`subtitle_safe`、`headline_rise`、`headline_pop` 和 `headline_drop` 都固定使用已验收的淡出。
- Agent 审阅卡显示 cue 的入场与出场；描边、阴影、背景卡片和剪映内置字体资源继续标为 `local_preview_only`，等待逐项视觉验收。

## 模型文本配方决策（2026-08-12）

`get_text_capabilities` 的每个文本预设现在包含 `selectionHint`。Agent 在首次文本创作前必须先读取目标 `get_timeline` 与能力目录；如果时间线不足以判定画面语义，再读取 storyboard。选择规则固定为：对白或旁白使用 `subtitle_safe`，递进/开场揭示使用 `headline_rise`，反差/意外/关键结果使用 `headline_pop`，结论/规则/警示使用 `headline_drop`。同一视觉 beat 至多一个 headline，不可把 headline 当普通字幕，也不能在用户未明确接受 local preview 时选用 `callout_card` 或 `cta_card`。

该规则只引导模型的创作选择；`replace_text_tracks` 仍是唯一有副作用的入口，Rust 仍解析并校验最终配方、时间和 Jianying 兼容性。

## 文本轨 QA（2026-08-12）

文本轨写入后会把阅读密度、超过两行、入/出场动画占满 cue 和相邻重复文案作为非阻断 `qualityWarnings` 回读模型；这样模型可在同一受限循环内决定压缩文案或延长 cue。跨文本轨的 headline 时间重叠改为拒绝，落实每个视觉 beat 只能有一个 headline。当前没有主体位置的真实证据，因此不声称已经检测文字遮挡人物。
