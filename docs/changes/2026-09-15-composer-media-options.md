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
