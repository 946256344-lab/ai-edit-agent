# 2026-08-18: 添加路由决策诊断日志

## 变更类型

feature（日志增强）

## 触发规则

- desktop-contract（修改 `src-tauri/src/agentloop/runtime.rs`）

## 问题背景

用户报告 storyboard 生成失败，错误信息为 `goal_must_be_question/storyboard/...`，来自 `runtime.rs:236` 的路由验证失败。该错误表明：
1. `fast_goal` 函数未能从用户请求中识别出明确的 storyboard 意图（返回 `None`）
2. 模型在路由决策时也未正确填写 `goal` 字段
3. `pinned_goal.or(declared)` 都是 `None`，导致验证失败

但从现有日志无法确定：
- 模型返回了什么 `goal` 值（`null`、空字符串、拼写错误、还是不在枚举范围内的值）
- 模型返回的 `route`、`isQuestion`、`tool` 是什么
- `fast_goal` 识别到的 `pinned_goal` 是什么
- 纠偏重试后模型是否修正了响应

缺少这些诊断信息导致无法快速定位问题根源是在 `fast_goal` 关键词识别、模型理解能力、还是 prompt 指令不清晰。

## 变更范围

### 新增日志（src-tauri/src/agentloop/runtime.rs）

**首次路由决策日志（line 163-172）**：
```rust
// 日志记录路由决策的原始值
log::info!(
    "Route decision received: route={}, goal={:?}, isQuestion={:?}, tool={:?}, pinnedGoal={:?}",
    response.route,
    response.goal,
    response.is_question,
    response.tool,
    pinned_goal.map(|g| g.code())
);
```
- 记录模型返回的原始 `route`、`goal`、`isQuestion`、`tool` 字段值
- 记录 backend 识别的 `pinned_goal`（通过 `fast_goal` 函数从用户请求中提取）
- 使用 `{:?}` 格式化 Option 类型，`None` 会显示为 "None"，`Some(value)` 会显示为 "Some(value)"

**纠偏重试后的日志（line 190-197）**：
```rust
// 日志记录纠偏后的值
log::info!(
    "Route correction received: route={}, goal={:?}, isQuestion={:?}, tool={:?}",
    response.route,
    response.goal,
    response.is_question,
    response.tool
);
```
- 记录纠偏提示后模型返回的修正值
- 对比首次决策和修正后的差异，判断纠偏是否有效

**验证失败时的警告日志（line 233-243）**：
```rust
let goal = pinned_goal.or(declared)
    .ok_or_else(|| {
        log::warn!(
            "Route validation failed: goal parsing failed. raw_goal={:?}, isQuestion={:?}, pinned={:?}",
            response.goal,
            response.is_question,
            pinned_goal.map(|g| g.code())
        );
        "route=run: goal must be question/storyboard/timeline/preview/jianying.".to_owned()
    })?;
```
- 当 `pinned_goal.or(declared)` 为 `None` 时触发
- 记录导致验证失败的原始 `goal` 字段值、`isQuestion` 值和 `pinned_goal`
- 使用 `log::warn!` 级别，因为这是一个需要纠正的异常情况

## 日志输出示例

**正常情况（fast_goal 识别成功）：**
```
[INFO] Route decision received: route=run, goal=None, isQuestion=Some(false), tool=None, pinnedGoal=Some("storyboard")
[INFO] Executing skill: generate_storyboard
```
- 模型未填写 `goal` 字段（`goal=None`），但 backend 通过关键词识别到了 `pinnedGoal=Some("storyboard")`
- 验证通过，执行 storyboard 生成

**模型漏填且 fast_goal 未识别（触发纠偏）：**
```
[INFO] Route decision received: route=run, goal=None, isQuestion=Some(false), tool=None, pinnedGoal=None
[WARN] Route validation failed: goal parsing failed. raw_goal=None, isQuestion=Some(false), pinned=None
[INFO] Route correction received: route=run, goal=Some("storyboard"), isQuestion=Some(false), tool=None
[INFO] Executing skill: generate_storyboard
```
- 首次决策：模型和 backend 都未识别目标（`goal=None`, `pinnedGoal=None`）
- 触发纠偏，系统向模型反馈错误
- 纠偏后：模型补充了 `goal=Some("storyboard")`
- 验证通过，执行 storyboard 生成

**模型返回不合法 goal 值：**
```
[INFO] Route decision received: route=run, goal=Some("storyboard_generation"), isQuestion=Some(false), tool=None, pinnedGoal=None
[WARN] Route validation failed: goal parsing failed. raw_goal=Some("storyboard_generation"), isQuestion=Some(false), pinned=None
[INFO] Route correction received: route=run, goal=Some("storyboard"), isQuestion=Some(false), tool=None
[INFO] Executing skill: generate_storyboard
```
- 模型返回了不在枚举范围内的 `goal` 值（`"storyboard_generation"` 而非 `"storyboard"`）
- `parse_declared_goal` 解析失败，返回 `None`
- 触发纠偏，模型修正为正确的 `"storyboard"`

**纠偏失败（两次都未识别）：**
```
[INFO] Route decision received: route=run, goal=None, isQuestion=Some(false), tool=None, pinnedGoal=None
[WARN] Route validation failed: goal parsing failed. raw_goal=None, isQuestion=Some(false), pinned=None
[INFO] Route correction received: route=run, goal=None, isQuestion=Some(false), tool=None
[WARN] Route validation failed: goal parsing failed. raw_goal=None, isQuestion=Some(false), pinned=None
[ERROR] Agent skill execution failed: route=run: goal must be question/storyboard/timeline/preview/jianying.
```
- 首次决策和纠偏后都未能识别目标
- 最终验证失败，返回错误给用户

## 向后兼容

- 只新增 `log::info!` 和 `log::warn!` 调用，不改变任何执行逻辑
- 日志级别选择：info 用于正常路径记录，warn 用于异常但可恢复的情况
- 日志内容不包含敏感信息（用户消息内容、项目 ID、凭据等）

## 同步文档

- **docs/architecture.md**：在维护记录中新增 2026-08-18 条目，记录路由决策诊断日志的位置和用途
- **docs/api.md**：在维护记录中新增 2026-08-18 条目，确认公开 Tauri 命令不变
- **TASKS.md**：将路由决策日志任务移至当前任务窗口标记区

## 公开契约

无变更。

## 验证证据

- ✅ Rust 库测试：113 passed
- ✅ Rust fmt/check：通过
- ✅ harness:test：通过
- ⏳ 实际 storyboard 生成场景验证：需要用户提供报错时的完整日志以确认诊断效果

## 后续任务

1. **收集实际失败案例的完整日志**：当用户再次遇到相同错误时，新增的日志会显示模型返回的原始 `goal` 值和 `pinned_goal`，用于定位问题根源
2. **增强 fast_goal 识别能力**（如果日志显示 `pinnedGoal=None` 频繁出现）：添加更多触发关键词组合，降低对模型准确性的依赖
3. **优化路由 prompt**（如果日志显示模型经常返回不合法的 `goal` 值）：更明确地列举合法的 goal 枚举值和判断条件
