# 改进路由产物边界理解与目标推理声明

**日期**：2026-08-17  
**类型**：improvement  
**影响范围**：`src-tauri/src/agentloop.rs`、`src-tauri/src/taskrouter.rs`、`.harness/architecture-budgets.json`

## 问题

用户发出"整理出视频的字幕文案"，Agent 误将其路由为 storyboard 生成目标，执行了 `generate_storyboard`，而非用户期望的字幕文案整理（timeline 文本轨编辑）。

根因分析：
1. `fast_goal()` 对"整理"无关键词命中，返回 `None`，pinned goal 为 `pending`
2. 路由 prompt 未说明产物边界职责，仅列举"storyboard/timeline edits"并列
3. 模型看到"字幕文案"联想到 storyboard 包含剧本内容，自由选择 `goal=storyboard`

同期问题：任务路由器（taskrouter）对 `create_new` 的触发阈值与 `continue_current` 相同，导致子任务请求（如"整理字幕"）有时被误判为新会话。

## 修改内容

### 方案 A：产物边界说明（agentloop.rs prompt）

在路由 prompt 中新增明确的产物边界职责说明：

- **storyboard**：从原始媒体选镜、构建叙事结构，是第一个创作步骤
- **timeline**：编辑已选镜头的结构，包含：时长调整、顺序重排、文本轨（字幕/文案）、音乐轨
- **preview**：将时间线渲染为可播放视频
- **jianyingDraft**：导出到剪映 Pro

并明确指出：
- 文本/字幕编辑（字幕、配音文本、文案整理）属于 `goal=timeline + replace_text_tracks`，不是 storyboard
- 音乐编辑属于 `goal=timeline + replace_music_tracks`，不是 storyboard

### 方案 B：结构化目标推理声明（agentloop.rs schema）

新增 `goalReasoning` 字段到 `ConversationRouteResponse`，要求：
- 当 `route=run` 且 goal 未被 `fast_goal()` pinned 时，模型必须提供 `goalReasoning`
- 验证 `goalReasoning` 内容中确实提及了所选产物边界的关键词
- 验证失败时触发一次纠偏重试，模型看到原因后修正

### 差异化路由置信度阈值（taskrouter.rs）

将单一的 `AUTO_ROUTE_CONFIDENCE = 0.85` 拆分为三个差异化阈值：

```rust
const CONTINUE_THRESHOLD: f64 = 0.70;   // 延续现有任务要求较低
const SWITCH_THRESHOLD: f64 = 0.90;     // 切换任务需要高置信度
const CREATE_NEW_THRESHOLD: f64 = 0.95; // 创建新任务需要非常明确
```

同时更新 taskrouter prompt，明确 `continue_current` 为默认行为，`create_new` 仅在用户明确要求"新的视频"/"新项目"时触发。

## 架构预算

- `agentloop.rs`：3654 → 3684 行（+30，产物边界说明）
- `taskrouter.rs`：53934 → 54818 字符（+884，路由逻辑增强）

增长来自产物边界知识说明和目标推理验证逻辑，是架构清晰化，不是代码膨胀。

## 同步文档

- `TASKS.md`：本次变更已记录为当前活动任务
- `docs/changes/`：本文件

## 验证

- `cargo test --lib`：130 个 Rust 库测试全部通过
- `cargo check`：无新增警告（仅既有 `PartiallyDone` dead_code）
- `cargo fmt --check`：通过
- `npm run harness:check`：全部通过（架构预算、Agent 契约、文档同步）

## 预期效果

- "整理出视频的字幕文案"→ 路由到 `goal=timeline`，执行 `replace_text_tracks` 或 `get_timeline` 观察
- "添加背景音乐"→ 路由到 `goal=timeline`，执行 `replace_music_tracks`
- "整理字幕/调整节奏"等子任务→ `continue_current`，保持在同一会话内
- 只有用户明确说"新视频"/"新项目"时，置信度 ≥ 0.95 才 `create_new`
