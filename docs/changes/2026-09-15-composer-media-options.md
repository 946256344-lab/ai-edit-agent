# 对话区媒体开关

分支：`codex/composer-media-options`。

## 结果

- 输入框底部加入配音、字幕、BGM 三个独立开关；按用户后续要求，新会话改为默认全开，选中显示勾选标志。
- 按后续视觉反馈改为紧凑圆角按钮：统一线性图标、低饱和淡紫选中底色、独立圆形勾选状态与轻量悬停反馈；输入区与按钮区用细分隔线区分。
- 视觉复查覆盖 480px 与 340px 容器，窄宽度自动收紧间距与图标尺寸；三个开关和发送按钮保持单行，开关状态可独立区分。
- 本轮选择随 `submit_conversation_turn` 保存到任务输入，关联实际用户消息；切换会话恢复选择，历史消息展示冻结的设置摘要。未发送的改动保留在会话内存草稿，重启恢复最近已发送选择。
- 原始用户文案保持不变，开关作为独立字段传递，不破坏任务归属 receipt。普通问答不触发生成，明确文字要求可以覆盖自动添加默认。
- 分镜 JSON 保存媒体设置快照，旧分镜无快照保持原行为。配音关闭跳过提前合成和后续自动配音；字幕关闭跳过分镜与配音对齐字幕。配音开启且只提供主题时 Phase 1 创作旁白，完整稿保持照念路径。
- BGM 复用已有选曲与写轨工具，写入后重新生成预览；新选音乐在有旁白时使用 0.15 音量。现有在线音乐配置、许可与剪映限制不变。
- 第一版仅开关，无新增音色/风格下拉、Provider、数据库表或自动删除轨道。

契约：`docs/api.md`。决策：`docs/decisions.md`。职责地图：`docs/codebase/STRUCTURE.md`。

## 验证

- 前端 build、lint、分支检查通过；lint 有原有 `StudioWorkspace.tsx` 常量比较警告。
- Rust check 通过；完整库测试 352 项中 351 通过。唯一失败是既有 `every_advertised_text_recipe_is_accepted_by_the_text_track_validator`，`subtitle_newsbar` 样式超界；本次未修改该测试或对应校验逻辑。
- 媒体快照覆盖全部 8 种开关组合及旧分镜兼容，Native 参数用例验证 `mediaOptions` 与独立 `includeSubtitles`。
- 两项 Agent 契约集成测试、Rust 格式检查、文档同步与 `git diff --check` 通过。
- 临时页面使用真实 `AgentWorkspace` 和 controller 验证默认全关、独立开关、会话隔离、发送摘要冻结；已移除临时页面。完整应用浏览器入口要求 Tauri，未将浏览器组件验证当成真实成片验收。
- `harness:test` 的文档检查测试使用旧 `store.rs` 规则，与当前策略不符；`harness:check` 的 Agent 边界检查不接受既有 `outbound_http.rs`。这两组文件与 `origin/master` 相同，未扩大本次修复范围。
- 未执行真实 Provider 配音/选曲和端到端成片验收。

## 原长期文档补充：2026-09-15：对话媒体开关

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

`submit_conversation_turn` 增加可选 `mediaOptions: { voiceover: boolean, subtitles: boolean, bgm: boolean, aspectRatio?: '9:16' | '16:9' | '1:1' }`（2026-09-27 起前端以画幅选择替代字幕开关，`subtitles` 随 `voiceover` 发送）。前端新会话默认全开，选中显示勾选标志；发送时冻结选择，`agent_tasks.input_json` 保存 `mediaOptions` 和 receipt 对应的 `userMessageId`。会话恢复读取最近一次发送设置，消息摘要按 `userMessageId` 关联。关闭项不自动新增轨道，不表示删除已有轨道；本轮明确的自然语言指令优先，普通问答不触发编辑。

Native `generate_storyboard` 增加 `mediaOptions`（null 沿用本轮选择，非 null 为模型按明确文字要求合并后的选择）和 `requestedDurationMs`（用户点名的成片毫秒，null 表示没说）；`synthesize_voiceover` 增加 `includeSubtitles`（null 沿用本轮字幕选择）。严格工具 Schema 包含这些 nullable 参数，旧的内部调用省略时仍使用默认路径。

分镜 `content_json.mediaOptions` 保存生成快照，无新增表或列。关闭配音跳过 audio-first 与后续自动配音；关闭字幕跳过分镜字幕和配音对齐字幕写入。配音开启时必须配音：已有可念稿则照念生成；只有主题时 Agent 先写旁白稿并征求同意，同意后再生成并合成。完整文案仍照稿念。BGM 由 `generate_storyboard` 在配音之后直接写入（2026-09-27 起；素材库用户音频优先，其次 Jamendo 器乐，都不可用时在 `mediaNotApplied.bgm` 说明原因），有旁白时音量为 0.15，无旁白为 0.35；许可与剪映交付限制沿用现有音乐能力。旧分镜无快照时保持既有行为。见 `docs/changes/2026-09-15-composer-media-options.md`、`docs/changes/2026-09-16-voiceover-script-consent.md`。
