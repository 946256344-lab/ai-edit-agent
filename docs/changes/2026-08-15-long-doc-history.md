# 长期文档历史补充

原实现与验证记录按当时文档保留，不代表当前契约。

## 原长期文档补充：2026-08-15：维护记录补充

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

维护记录（2026-08-15）：preview render_preview 命令不变；render_timeline_clip 内部实现修复 -t 参数截断，不影响公开 API。
维护记录（2026-08-15）：公开 Tauri 命令不变；agentloop/taskrouter 内部路由验证新增 validate-then-correct 重试，不影响命令签名或 schema。
维护记录（2026-08-16）：公开命令签名与 schema 不变；内部错误路径改为输出真实错误日志而非静默 fallback，调用方可观察到更准确的失败状态与错误码。
维护记录（2026-08-18）：公开 Tauri 命令不变；storyboard 生成内部新增详细日志输出（入口参数、素材库存统计、素材样本、候选排序、多模态内容构建、模型请求/响应、重试进度、归一化修正、验证结果等），覆盖 `generate_storyboard_internal`、`request_storyboard` 和 `normalize_storyboard_candidate` 共 15 处日志点，用于诊断选镜与验证失败及数据库分类与文件系统不一致等异常，不影响公开 API 签名或返回值结构。
维护记录（2026-08-18）：修复素材 relink 和分析回写时 kind 字段未同步更新的数据一致性问题。confirm_asset_relink 命令签名不变，内部行为变化为：relink 时从新 source_reference 重新计算 kind 字段并同步更新到数据库；update_analysis_status 在分析结果回写时也会同步验证并更新 kind。修复后，用户将图片素材替换为视频并 relink 时，数据库 kind 字段会正确从 "image" 更新为 "video"，避免数据库分类与文件系统不一致。公开命令参数、返回值和 SQLite schema 不变，纯内部实现修复。
维护记录（2026-08-18）：公开 Tauri 命令不变；agentloop/runtime.rs 路由决策新增三处诊断日志（首次决策、纠偏修正、验证失败），记录模型原始 route/goal/isQuestion/tool 值和 backend 的 pinnedGoal，不改变命令签名或 ConversationRouteResponse schema。
维护记录（2026-08-18）：公开 Tauri 命令不变；agentloop/runtime.rs::decide_conversation_route 的路由决策 prompt 明确列举 5 个合法 goal 枚举值（question, storyboard, timeline, preview, jianying）和对应推荐工具，修复模型漏填 goal 字段或返回不合法值导致的路由验证失败。Prompt 改进不改变 ConversationRouteResponse schema、命令签名或工具白名单。
维护记录（2026-08-18）：公开 Tauri 命令不变；storyboard/phases.rs::phase3_fine_edit 的 Phase 3 prompt 补充 matchLevel 枚举约束（"matchLevel must be 'direct' or 'contextual'"），与 Phase 2 保持一致，防止独立模型调用返回其他字符串导致验证失败。Prompt 改进不改变 StoryboardContent schema、命令签名或工具白名单。
维护记录（2026-08-20）：公开 Tauri 命令不变；agentloop/prompt.rs::load_native_message_history 内部函数新增 `editing_task_id` 参数，查询改为 JOIN `conversations` 表并同时验证 `conversation_id` 和 `editing_task_id`，确保严格会话隔离，防止跨会话数据泄漏。负向回归：conversation 与 editing_task 不匹配时历史必须为空。修改仅影响 Rust 内部 API，不改变任何 Tauri 命令签名或前端接口。
维护记录（2026-08-20）：`resolve_conversation_task` 命令签名不变；候选从最近 12 个任务改为仅当前激活任务，路由模型不再接收兄弟任务的 title/brief/`active_subgoal`，也不再按名称切换已有任务。没有激活任务时直接创建新任务。澄清文案不再列举其他任务名称。
维护记录（2026-08-27）：公开 Tauri 命令、参数、返回值与 SQLite schema 不变；Rust 后端 dead-code 清理删除未使用的旧 storyboard 入口与 Provider 包装函数，不影响 Provider 协议或 Agent 工具白名单。见 docs/changes/2026-08-27-cleanup-rust-warnings.md。
维护记录（2026-08-31）：Agent `generate_storyboard` 成功后自动串联 timeline 与 preview；前端移除 storyboard 确认 UI 与 `confirmStoryboardAndPreview` invoke。Task Resolver 低置信度默认继续当前任务。`confirm_storyboard_and_preview` Tauri 命令保留兼容。见 docs/changes/2026-08-31-streamline-storyboard-to-preview.md。
维护记录（2026-09-01）：公开 Tauri 命令与 SQLite schema 不变；storyboard Phase 3 改为「模型全局主创 + Rust 结构化修复包回传」。`enforce_phase3_scope` 重写为 `collect_phase3_issues`，一次收集候选的全部结构性问题（beat 乱序、越池素材、beat 内重复素材、首镜头被换、uncovered beat 被补镜头等）并附 `allowedChanges`，`needs_model_decision=false` 的问题（请求失败、可机械修正的字段）不入模型、其余打包成 `RepairPacket` JSON 注入下一轮 Phase 3 prompt，模型只修被点名的镜头、不重写整条 storyboard；纯机械的子镜头字段标准化仍在 Rust 内无条件执行。主要失败信息归一到修复包第一条 issue 的 message，供任务终态与日志使用。见 docs/changes/2026-09-01-phase3-repair-packet-loop.md。
维护记录（2026-09-01）：公开 Tauri 命令与 SQLite schema 不变；RepairPacket 升级为 agent repair loop 的完整上下文：① `frozenShots`——未被问题点名的镜头视为已确认正确，prompt 明确「保持不动，除非修复其他问题确需改动」；② `previousShots`——模型每次 Phase 3 是独立请求无对话上下文，修复包携带上一轮候选的精简快照（序号/beat/asset/时长/源范围），模型据此"接着改"而不是重写全局；③ `repairHistory`——记录每轮修过什么问题类型、涉及哪些镜头、是否已解决，提示模型不要回退已修复内容；prompt 措辞为"你是编辑、规则是边界不是微指令"，避免限制模型发挥。见 docs/changes/2026-09-01-phase3-repair-packet-loop.md。
维护记录（2026-09-03）：公开契约不变；Storyboard 改为五阶段（本地 Top-12 去重补位 → 选 2–3 → 精修时间段 → 校验），单步重试分传输/语义预算，失败带 `partialCandidateSummary`，debug 可开 `STORYBOARD_PROVIDER_TRACE`。见 `docs/changes/2026-09-03-storyboard-select-then-refine.md`。
维护记录（2026-09-03）：本地安全上限抬至 100 镜/beat；短 brief 偏 key_message；Phase5 结构失败不回 Phase4。见 `docs/changes/2026-09-03-storyboard-shot-cap-and-short-brief.md`。
维护记录（2026-09-04）：`key_message` 收敛为 ≤15s 短视频（默认 8–15s、2–5 beat）。见 `docs/changes/2026-09-04-key-message-15s-cap.md`。
维护记录（2026-09-04）：Phase 3 附带关键帧网格选片；Phase 4 用导入关键帧建粗窗再段内精修（不确定加密 + 旁白时长托底；修窗尾 clamp panic）。见 `docs/changes/2026-09-04-phase3-4-keyframe-inspect.md`。
维护记录（2026-09-04）：可念稿强制 full_script+audio-first；normalize/join 去重旁白；key_message 旁白硬门。见 `docs/changes/2026-09-04-voiceover-narration-contract.md`。

## 原长期文档补充：2026-08-15：维护记录补充

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

维护记录（2026-08-15）： 的 FFmpeg `-t` 参数改为 `min(source_range, timeline_slot)`，防止源素材短于时间线槽位时生成黑帧。测试模块提取为独立 `preview_tests.rs`，`preview.rs` 预算从 1015 降至 608 行。

## 原长期文档补充：2026-08-15：维护记录补充

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

维护记录（2026-08-15）：render_timeline_clip 的 FFmpeg -t 参数改为 min(source_range, timeline_slot)，防止源素材短于时间线槽位时生成黑帧；测试模块提取为独立 preview_tests.rs，preview.rs 预算从 1015 降至 608 行。
维护记录（2026-08-15）：agentloop.rs::decide_conversation_route 和 taskrouter.rs::resolve_conversation_task 新增 validate-then-correct 重试逻辑；路由验证失败时把错误原因反馈给模型后重试一次，不改变公开命令或运行时边界。
维护记录（2026-08-16）：移除 agent.rs、agentloop.rs、assets.rs 中所有静默 fallback（遇错返回硬编码合成值），补全被丢弃的真实错误日志；降级路径仍封闭失败，不伪造成功结果，公开命令与运行时边界不变。
维护记录（2026-08-18）：agentloop/runtime.rs::decide_conversation_route 新增三处诊断日志（首次路由决策、纠偏后修正值、验证失败上下文），记录模型返回的原始 route/goal/isQuestion/tool 和 backend 识别的 pinnedGoal，用于诊断路由验证失败的根本原因（fast_goal 关键词识别遗漏、模型返回不合法 goal 值、还是 prompt 指令不清晰），不改变执行逻辑或公开命令。
维护记录（2026-08-18）：实现四阶段 storyboard 选镜优化系统。models.rs 新增 visual_quality_score 和 scene_duration_ms 字段（Option 类型向后兼容）；新增 storyboard/scoring.rs 独立评分模块（质量/时长/语义/多样性/新鲜度综合评分），request_storyboard 只向模型提供 top-5 候选；新增多样性硬门（连续禁止、同素材≤40%）；新增 storyboard/semantic.rs 和 validation.rs 架构层（接口定义，实现体 TODO）。公开命令与 schema 不变。
维护记录（2026-08-18）：新增 storyboard/multimodal.rs 多模态选镜架构层。定义关键帧网格配置（4-8 帧拼成 2x2/2x4 网格）、generate_keyframe_grid（FFmpeg I 帧提取 + image crate 拼图）和 build_multimodal_content（base64 编码图像块）接口。request_storyboard 检测 keyframe_grid_path 并构建多模态输入，让模型直接从关键帧画面判断语义匹配度。当前阶段接口定义完成，FFmpeg 和 base64 实现体 TODO。公开命令与 schema 不变。
维护记录（2026-08-18）：修复素材 relink 和分析回写时 kind 字段未同步更新的数据一致性问题。confirm_asset_relink 在两个 UPDATE 分支（preserve_analysis = true/false）中新增 kind = ? 字段更新，从新 source_reference 重新计算 kind；update_analysis_status 在分析结果回写时同步重新计算并更新 kind 字段。修复后，用户将图片素材替换为视频并 relink 时，数据库 kind 字段会正确同步为 "video"，避免 storyboard 生成日志显示的素材类型与文件系统不一致。公开命令行为不变（仅内部实现修复），SQLite schema 不变。
维护记录（2026-08-18）：agentloop/runtime.rs::decide_conversation_route 的路由决策 prompt 明确列举 5 个合法 goal 枚举值（question, storyboard, timeline, preview, jianying）和对应推荐工具，修复模型漏填 goal 字段或返回不合法值（如 "storyboard_generation"）导致的路由验证失败。Prompt 从模糊描述（"Include goal"）改为明确枚举列表 + 工具映射，降低模型猜测错误的概率。不改变 ConversationRouteResponse schema、公开命令或运行时边界。
维护记录（2026-08-18）：storyboard/phases.rs::phase3_fine_edit 的 Phase 3 prompt 补充 matchLevel 枚举约束。Phase 3 是独立模型调用，原 prompt 只说 "Each shot must contain: ... matchLevel" 未列举合法值；现补充 "matchLevel must be 'direct' (evidence visibly supports the beat) or 'contextual' (honest scene-setting)"，与 Phase 2 prompt 保持一致，防止模型返回其他字符串（如 "high"、"medium"）导致验证失败。不改变 StoryboardContent schema、公开命令或运行时边界。
维护记录（2026-08-19）：为诊断启动卡顿问题，在 projects.rs::initialize_local_store（10 处）、assets/analysis.rs::resume_incomplete_analysis（7 处）、assets/visual.rs::recover_interrupted_visual_batches（3 处）和 backfill_queued_visual_batches（6 处）添加 [PERF] 前缀的性能日志，测量数据库连接、清理中断任务、恢复分析批次、启动后台 worker 等关键步骤的实际耗时。所有日志使用 log::info! 级别，使用 std::time::Instant 计时。只添加诊断日志，不改变执行逻辑、公开命令或 SQLite schema。
维护记录（2026-08-19）：优化 projects.rs::recover_missing_agent_completion_messages 查询性能。用窗口函数（ROW_NUMBER() OVER PARTITION BY）+ CTE 替代相关子查询，避免对每行外部结果重新执行一次子查询的 O(N²) 复杂度。原查询在有几百条任务记录时耗时 ~300ms（占启动时间 80%），优化后预期降至 <20ms。查询语义完全等价（最新任务判定逻辑、NOT EXISTS 判定逻辑保持不变），不影响公开命令或 SQLite schema。SQLite 3.25+ 支持窗口函数，Tauri 自带 SQLite 3.45+ 满足要求。
维护记录（2026-08-19）：Provider 新增协议无关的 ModelTurn/ModelOutputItem/FunctionCall。Responses 完整 response.output、Responses SSE item、Chat Completions 普通响应与 SSE tool-call 增量均可解析；自定义 Chat 适配器保留 tools/tool_choice/parallel_tool_calls，并将函数调用历史映射为 assistant.tool_calls 与 tool_call_id。Legacy Runtime、Router、LoopGoal 和副作用流程不变，store:false 不影响 output item 保留。
维护记录（2026-08-27）：`agentloop/tools.rs` 集中维护完整工具目录（含常驻控制工具 `load_tools` 与普通只读诊断工具 `read_logs`）、主链及交付工具；Provider 每轮只接收 `load_tools` 与最多 5 个已加载业务 schema。每项使用 strict JSON Schema 与 additionalProperties=false；严格 schema 的所有属性都列入 required，语义可选值使用 nullable 类型并由 Native loop 再次校验长度、范围和枚举。模型不携带 projectId、conversationId 或本地路径；领域执行仍由既有 apply_skill 负责，许可证、文字兼容矩阵、确认门与领域算法不移入工具适配层。
维护记录（2026-08-27）：Rust 后端 dead-code 警告清理；删除旧单阶段 `request_storyboard` 与场景检测遗留代码，三阶段 storyboard 生产链路不变。见 docs/changes/2026-08-27-cleanup-rust-warnings.md。
维护记录（2026-09-03）：Storyboard 改为五阶段选片/精修分离，单步重试与 `STORYBOARD_PROVIDER_TRACE`。见 `docs/changes/2026-09-03-storyboard-select-then-refine.md`。
维护记录（2026-09-03）：安全上限 100 镜/beat；短 brief 时长收敛；Phase5 路由。见 `docs/changes/2026-09-03-storyboard-shot-cap-and-short-brief.md`。
维护记录（2026-09-04）：`key_message` 默认 ≤15s（8–15s、2–5 beat）。见 `docs/changes/2026-09-04-key-message-15s-cap.md`。
维护记录（2026-09-04）：可念稿强制 full_script+audio-first；旁白去重与硬门。见 `docs/changes/2026-09-04-voiceover-narration-contract.md`。
维护记录（2026-09-15）：素材切段只认已验证硬切，无切不切。见 `docs/changes/2026-09-15-hard-cut-segments.md`。
维护记录（2026-09-15）：硬切片段内用帧差运动能量收缩可用窗。见 `docs/changes/2026-09-15-motion-energy-trim.md`。
维护记录（2026-09-16）：素材详情展示片段运动能量曲线。见 `docs/changes/2026-09-16-motion-energy-detail.md`。
维护记录（2026-09-18）：技术分析先扫关键帧，缩略图/抽帧超时不整条失败。见 `docs/changes/2026-09-18-faster-asset-analysis.md`。
