# 2026-08-07：多轮对话记忆与模型意图分类

## 变更

- `src-tauri/src/agentloop.rs`：目标派生从“纯关键词规则”改为两级。新增 `fast_goal` 确定性快路径（`EDIT_VERBS`/`CREATE_VERBS`/`QUESTION_PHRASES`：明确产物命令、明确编辑动词、清晰提问直接判定；疑问且无动作词归问答，疑问且带动词留给模型；`EDIT_VERBS` 不再含“镜头”等名词，避免把真实提问误判为时间线编辑）。快速路径无法确定的请求才通过 `classify_goal_with_model` 消耗一次轻量模型调用（携带对话历史，输出 `goal` + `isQuestion`，`isQuestion` 为真时一律归为问答；分类失败默认问答）。`derive_loop_goal` 签名从 `(request)` 改为 `(access, request, history)`。
- 新增多轮记忆：`load_message_history` 从 `messages` 表按 `conversation_id` 读取最近消息（最多 12 条、总字符预算 8000，排除当前请求本身，按时间正序），`render_history` 渲染为带说话人标签的文本；`LoopState` 增加 `history` 字段，`build_step_prompt`/`classify_goal_with_model` 均携带这段会话历史，让模型能依据上文回答“你觉得呢”这类多轮追问。
- 明确不带“模型不可用”的多层兜底：模型分类失败仅回退到问答目标；保留 `agent.rs` 现有的友好降级文案。
- 补充（同批）：为所有经 Agent 循环发出的模型请求加上限时，杜绝一次请求因 Provider 不响应无限挂起——`agentloop.rs` 的 `run_step` 带 `AGENT_STEP_TIMEOUT`（120 秒）、`classify_goal_with_model` 带 `CLASSIFY_TIMEOUT`（30 秒）；`storyboard.rs` 的 `generate_storyboard` 带 `STORYBOARD_TIMEOUT`（120 秒），沿用 `assets.rs` 视觉分析 30 秒超时的 `post_model_payload` 模式。超时即按失败保存安全结果并返回固定诚实降级回复。
- 补充（同批）：`run_agent_loop` 不再把 `run_step` 的模型/provider 失败当作硬错误冒泡到客户端（此前会落到 `agent.rs` 通用"受限操作没有完成"失败文案，对纯提问是误导），而是按目标返回 `model_unavailable_message` 的诚实降级回复（问答目标说明"模型没有返回解释"；产物目标说明"本轮没有产出/修改对应交付物，请检查模型连接后重试"），并保持 `agent_tasks` 安全失败记录。真实 Provider 挂起场景已实测：日志出现 `AI agent-loop step request failed: Custom API request failed before receiving a response.`，客户端收到的是目标相关的诚实降级文案而非技术错误。

## 同步文档

本变更同步了以下长期文档：`AGENTS.md`、`TASKS.md`、`docs/architecture.md`、`docs/api.md`、`docs/decisions.md`（新增 ADR-033）。

## 验证

- `cargo build --lib` 编译通过，无警告。
- `cargo test --lib` 25 通过、0 失败（新增 `fast_goal_pins_unambiguous_requests`/`fast_goal_answers_questions_instead_of_forcing_edits`/`fast_goal_leaves_ambiguous_requests_for_the_model`/`parsed_classification_prefers_a_truthful_question_flag`/`message_history_excludes_the_current_request_and_is_chronological`/`message_history_drops_other_conversations`/`render_history_labels_speakers_in_order`）。
- 前端无改动（`npm run lint`/`npm run build` 不在本次变更范围）。模型分类分支依赖真实 Provider 响应，待桌面手工验证“请告诉我选择每个镜头的逻辑”能返回自然回答而非固定诚实文案。