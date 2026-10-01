# 长期文档历史补充

原实现与验证记录按当时文档保留，不代表当前契约。

## 原长期文档补充：2026-08-20：独立维护记录

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

维护记录（2026-08-20）：公开 Tauri 命令不变；agentloop/prompt.rs::load_native_message_history 内部函数新增 `editing_task_id` 参数，查询改为 JOIN `conversations` 表并同时验证 `conversation_id` 和 `editing_task_id`，确保严格会话隔离，防止跨会话数据泄漏。负向回归：conversation 与 editing_task 不匹配时历史必须为空。修改仅影响 Rust 内部 API，不改变任何 Tauri 命令签名或前端接口。

## 原长期文档补充：2026-08-20：独立维护记录

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

维护记录（2026-08-20）：`resolve_conversation_task` 命令签名不变；候选从最近 12 个任务改为仅当前激活任务，路由模型不再接收兄弟任务的 title/brief/`active_subgoal`，也不再按名称切换已有任务。没有激活任务时直接创建新任务。澄清文案不再列举其他任务名称。
