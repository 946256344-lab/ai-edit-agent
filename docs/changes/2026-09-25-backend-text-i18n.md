# 2026-09-25：界面中英双语（第二步：Rust 返回文案与 Agent 回复语言）

## 结果

切到英文界面后，启动检查条、本地模型下载进度、编辑器名称与交付结果、模型设置里的连接状态、镜头候选不可用原因都显示英文；Agent 按界面语言回复，系统兜底回复（失败、停止、模型不可连接）也跟随界面语言。修复第一步引入的问题：英文界面新建的会话默认名 `New edit session` 现在也会在首条用户消息后自动改成请求摘要。

## 范围

- `release_readiness.rs`：每个检查项增加 `messageKey` 与 `messageParams`（磁盘空间带 `size`，CLIP 可用带 `vision/text`）。标题由前端按 `id` 翻译。
- `runtime_models.rs`：下载状态增加 `messageKey` 与 `messageParams`；`model` 参数给产物 id，`source` 为 `official|mirror`，由前端翻成当前语言的名称。
- `agent.rs`：`submit_conversation_turn` 新增可选 `uiLocale`，写入 `agent_tasks.input_json.uiLocale`；新增 `UiLocale` 与 `task_ui_locale`，四处系统兜底回复按任务语言选中英文。
- `agentloop/native.rs`：系统提示追加回复语言说明，并明确不得因此翻译用户的旁白、字幕或屏幕文字。
- `projects.rs`、`storyboard.rs`：占位标题判定加入 `New edit session`；抽出 `touch_after_message` 并补一条回归测试。
- 文档：`docs/api.md` 更新 `submit_conversation_turn`、`get_release_readiness`、`get_runtime_model_status` 的字段说明；`docs/decisions.md` 第 14 节。
- 前端：`src/lib/i18n` 增加 `backend` 词典与 `translateKeyed`；编辑器名称/说明按 `id`、交付结果按 `editorId/status/displayName` 在前端拼出；OAuth 与自定义 API 的“已连接/等待登录”按 `state` 翻译；候选不可用原因按可用时长与原镜头时长区分。

## 禁止变化

- 中文界面显示与改动前逐字一致；Rust 的中文 `title/message` 原样保留，作为未知键的回落与日志。
- 旧任务、旧入口 `execute_agent_edit` 没有 `uiLocale` 时按简体中文，行为不变。
- 界面语言不改变成片语言：旁白、字幕与分镜文案仍跟随用户文案。
- 不改 SQLite schema、工具目录与工具参数。

## 边界

- 仍按原文显示：OAuth/自定义 API 的失败原因、模型下载失败的底层错误（作为参数嵌在译文里）、Rust 返回后由前端映射的其他错误原文。
- 任务路由的澄清问题由模型生成，不受界面语言约束。
- 语义召回仍用中文向量模型，英文素材与文案的召回质量另行评估。

## 验证

- `cargo check --tests`：通过，没有新增警告。
- `cargo test --lib -- projects:: release_readiness:: runtime_models:: agent:: taskrouter:: agentloop::`：新回归测试通过；`agentloop::native::tests::ordinary_question_returns_message_without_tool_call` 失败，是 97b078f 新增两个工具后工具数 29 与名单 27 不符的既有问题，与本次无关，已另行登记。
- `npx tsc -b`、`npm run lint`、`npm run harness:check`：通过。
- 浏览器模拟 IPC 下英文界面的启动检查条与模型下载条显示英文。
- 真实桌面下 Agent 英文回复与交付文案待验收。
