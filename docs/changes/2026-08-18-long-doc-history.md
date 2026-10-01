# 长期文档历史补充

原实现与验证记录按当时文档保留，不代表当前契约。

## 原长期文档补充：2026-08-18：独立维护记录

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

维护记录（2026-08-18）：公开 Tauri 命令不变；storyboard 生成内部新增详细日志输出（入口参数、素材库存统计、素材样本、候选排序、多模态内容构建、模型请求/响应、重试进度、归一化修正、验证结果等），覆盖 `generate_storyboard_internal`、`request_storyboard` 和 `normalize_storyboard_candidate` 共 15 处日志点，用于诊断选镜与验证失败及数据库分类与文件系统不一致等异常，不影响公开 API 签名或返回值结构。

## 原长期文档补充：2026-08-18：独立维护记录

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

维护记录（2026-08-18）：修复素材 relink 和分析回写时 kind 字段未同步更新的数据一致性问题。confirm_asset_relink 命令签名不变，内部行为变化为：relink 时从新 source_reference 重新计算 kind 字段并同步更新到数据库；update_analysis_status 在分析结果回写时也会同步验证并更新 kind。修复后，用户将图片素材替换为视频并 relink 时，数据库 kind 字段会正确从 "image" 更新为 "video"，避免数据库分类与文件系统不一致。公开命令参数、返回值和 SQLite schema 不变，纯内部实现修复。

## 原长期文档补充：2026-08-18：独立维护记录

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

维护记录（2026-08-18）：公开 Tauri 命令不变；agentloop/runtime.rs 路由决策新增三处诊断日志（首次决策、纠偏修正、验证失败），记录模型原始 route/goal/isQuestion/tool 值和 backend 的 pinnedGoal，不改变命令签名或 ConversationRouteResponse schema。

## 原长期文档补充：2026-08-18：独立维护记录

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

维护记录（2026-08-18）：公开 Tauri 命令不变；agentloop/runtime.rs::decide_conversation_route 的路由决策 prompt 明确列举 5 个合法 goal 枚举值（question, storyboard, timeline, preview, jianying）和对应推荐工具，修复模型漏填 goal 字段或返回不合法值导致的路由验证失败。Prompt 改进不改变 ConversationRouteResponse schema、命令签名或工具白名单。

## 原长期文档补充：2026-08-18：独立维护记录

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

维护记录（2026-08-18）：公开 Tauri 命令不变；storyboard/phases.rs::phase3_fine_edit 的 Phase 3 prompt 补充 matchLevel 枚举约束（"matchLevel must be 'direct' or 'contextual'"），与 Phase 2 保持一致，防止独立模型调用返回其他字符串导致验证失败。Prompt 改进不改变 StoryboardContent schema、命令签名或工具白名单。
