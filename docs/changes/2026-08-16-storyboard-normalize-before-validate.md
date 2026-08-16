# 修复 storyboard 校验前先规范化数值约束

日期：2026-08-16  
范围：`src-tauri/src/storyboard.rs`

## 目标

修复 `generate_storyboard_internal` 重试循环中校验失败无法收敛的问题：模型生成的
`duration_ms > source_range` 和 `total_duration ≠ target_duration_ms` 这两类纯数值
偏差被 `validate_storyboard` 反复拒绝并回发给模型，但模型无法仅凭文字反馈稳定修正
数值；三次重试耗尽后 `generate_storyboard` 以 `invalid_source_time_range` 失败，
致使新剪辑会话的第一条请求始终无法完成。

## 变更摘要

### storyboard.rs — generate_storyboard_internal

**修改前**：

```
for _ in 0..MAX_STORYBOARD_REVISIONS {
    match request_storyboard(...) {
        Ok(candidate) => match validate_storyboard(&candidate, ...) {
            Ok(()) => { content = Some(candidate); break; }
            Err(e) => { feedback = Some(e); previous = Some(candidate); }
        },
        Err(e) => { feedback = Some(e); }
    }
}
// 通过后才规范化
let content = content.map(|c| normalize_storyboard_candidate(c, ...)).ok_or_else(...)?;
```

`normalize_storyboard_candidate` 只在校验**通过后**作为最终整理步骤执行；校验
`shot.duration_ms > (source_end_ms - source_start_ms)` 和总时长偏差检查会被模型
原始输出触发，三次重试配额全部消耗在可自动修正的数值约束上。

**修改后**：

```
for _ in 0..MAX_STORYBOARD_REVISIONS {
    match request_storyboard(...) {
        Ok(candidate) => {
            // 先规范化（夹紧数值约束、修正 target_duration_ms），
            // 再校验结构性约束；纯数值偏差不再占用模型重试配额。
            let candidate = normalize_storyboard_candidate(candidate, &sources, brief);
            match validate_storyboard(&candidate, ...) {
                Ok(()) => { content = Some(candidate); break; }
                Err(e) => { feedback = Some(e); previous = Some(candidate); }
            }
        },
        Err(e) => { feedback = Some(e); }
    }
}
let content = content.ok_or_else(...)?;
```

`normalize_storyboard_candidate` 在校验前执行，自动：

- 将 `shot.duration_ms` 夹紧到实际源时间段宽度（`end - start`）
- 将 `content.target_duration_ms` 修正为规范化后的实际总时长

校验后剩余的失败全部属于结构性问题（非法 beat ID、资产不可用、图片非零源范围等），
模型重试配额只用于这类有意义的反馈。

## 不变边界

- 公开 Tauri 命令、输入/输出类型、SQLite schema、工具白名单不变
- `validate_storyboard` 本身逻辑不变；结构性校验仍由模型反馈修正
- 持久化的 `target_duration_ms` 现在反映规范化后的实际总时长，而非模型原始提案

## 同步文档

本次改动仅修改内部逻辑，不改变公开契约，以下文档无结构变化：

- docs/architecture.md（已确认：架构边界不变）
- docs/api.md（已确认：公开 Tauri 命令和工具不变）
- TASKS.md（已更新本条完成记录）

## 完成门

129 个 Rust 库测试 + Rust fmt/check 通过；不涉及前端、schema 或 Agent 工具变更。
