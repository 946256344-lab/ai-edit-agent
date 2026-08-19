# 2026-08-18: 在路由决策 prompt 中明确 goal 枚举值

## 变更类型

bugfix（prompt 改进）

## 触发规则

- desktop-contract（修改 `src-tauri/src/agentloop/runtime.rs`）

## 问题背景

用户报告 storyboard 生成失败，日志显示：
```
[INFO] Route decision received: route=run, goal=None, isQuestion=Some(false), tool=Some("generate_storyboard"), pinnedGoal=None
[WARN] Route validation failed: goal parsing failed. raw_goal=None, isQuestion=Some(false), pinned=None
```

**根本原因**：模型选择了 `tool=generate_storyboard`，但**没有返回 `goal` 字段**。路由验证要求 `route=run` 时必须有明确的 `goal`（通过 `pinnedGoal` 或模型声明的 `goal`）。

检查 prompt 发现：
- Prompt 要求模型"Include goal"，但**没有明确列举合法的枚举值**
- 模型只能从模糊的描述（"storyboard/timeline edits, preview, or Jianying delivery"）猜测应该返回什么字符串
- 结果：模型可能完全漏填 `goal`，或返回不合法的值（如 `"storyboard_generation"` 而非 `"storyboard"`）

## 变更范围

### 修改路由决策 prompt（src-tauri/src/agentloop/runtime.rs, line ~127-131）

**修改前**：
```rust
"Return one JSON object. route must be respond, clarify, or run.\n\
 For goal=question, include informationScope=general or project. ...\n\
 - run: ... Include goal, goalReasoning ..."
```

**修改后**：
```rust
"Return one JSON object. route must be respond, clarify, or run.\n\
 Valid goal values (required for route=run): question, storyboard, timeline, preview, jianying. Choose based on the artifact boundary above.\n\
 - goal=question: answering a question by observing project state (use informationScope=general or project)\n\
 - goal=storyboard: creating initial shot selection from raw media (first tool: generate_storyboard)\n\
 - goal=timeline: editing existing storyboard/timeline structure (tools: create_timeline_draft, replace_text_tracks, replace_music_tracks)\n\
 - goal=preview: rendering video preview (first tool: render_preview)\n\
 - goal=jianying: exporting to Jianying format (first tool: create_jianying_draft)\n\n\
 Route decision rules:\n\
 - respond: ...\n\
 - clarify: ...\n\
 - run: ... Include goal (one of the 5 valid values above), goalReasoning ..."
```

**改进点**：
1. **明确列举 5 个合法值**：`question, storyboard, timeline, preview, jianying`（与 `LoopGoal` 枚举的 `.code()` 输出完全对应）
2. **每个 goal 附带推荐工具**：帮助模型理解 goal 与工具的映射关系
3. **强调"required for route=run"**：明确 `route=run` 时必须填写 `goal`

## 向后兼容

- Prompt 变化不影响既有成功路径（模型本来就在返回 `goal`）
- 只改进失败路径（模型漏填或返回不合法值时，现在有明确的枚举列表作为约束）
- 不改变 `ConversationRouteResponse` schema、SQLite schema、工具白名单或公开命令

## 同步文档

- **docs/architecture.md**：在维护记录中新增 2026-08-18 条目，记录路由决策 prompt 改进
- **docs/api.md**：无需更新（prompt 内容不属于公开 API）
- **TASKS.md**：在当前任务窗口新增本项，标记为独立 prompt 改进

## 公开契约

无变更。

## 验证证据

- ✅ Rust 库测试：113 passed + 2 contract tests
- ✅ Rust fmt/check：通过
- ✅ harness:test：通过
- ⏳ 实际 storyboard 生成场景验证：需要用户重试失败请求，观察模型是否正确填写 `goal` 字段

## 后续任务

1. **收集实际效果**：用户重试失败请求时，观察日志中的 `goal` 字段是否正确填写
2. **增强 fast_goal 识别**（如果 prompt 改进后仍频繁触发验证失败）：添加更多关键词组合，降低对模型准确性的依赖
