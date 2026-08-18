# 2026-08-18: Phase 3 prompt 明确 matchLevel 枚举值

## 变更类型

bugfix（prompt 改进）

## 触发规则

- desktop-contract（修改 `src-tauri/src/storyboard/phases.rs`）

## 问题背景

三阶段 storyboard 生成中，Phase 2 的 prompt 已明确 `matchLevel` 枚举：
```
matchLevel must be 'direct' (evidence visibly supports the beat) or 'contextual' (honest scene-setting).
```

但 Phase 3 是独立的模型调用，prompt 只说 `"Each shot must contain: ... matchLevel"`，**没有重新列举合法值**。模型不能假设记得 Phase 2 的约束，可能返回其他值（如 `"high"`、`"medium"`、`"strong"` 等）导致验证失败。

**根本原因**：Phase 3 prompt 缺少枚举约束，模型可能返回任意字符串作为 `matchLevel`。

## 变更范围

### 修改 Phase 3 prompt（src-tauri/src/storyboard/phases.rs, line 199-201）

**修改前**：
```rust
"Return the complete final JSON with: title, summary, targetDurationMs, scriptMode, beats, uncoveredBeatIds, and shots.\n\
 Each shot must contain: orderIndex, durationMs, purpose, onScreenText, assetId, sourceStartMs, sourceEndMs, reason, beatId, matchLevel.\n\
 Do NOT add new assets — only refine timing and structure of the existing rough shots.\n\
 This is the FINAL pass before execution.",
```

**修改后**：
```rust
"Return the complete final JSON with: title, summary, targetDurationMs, scriptMode, beats, uncoveredBeatIds, and shots.\n\
 Each shot must contain: orderIndex, durationMs, purpose, onScreenText, assetId, sourceStartMs, sourceEndMs, reason, beatId, matchLevel.\n\
 matchLevel must be 'direct' (evidence visibly supports the beat) or 'contextual' (honest scene-setting).\n\
 Do NOT add new assets — only refine timing and structure of the existing rough shots.\n\
 This is the FINAL pass before execution.",
```

**改进点**：
- 在 Phase 3 prompt 中重新明确 `matchLevel` 的两个合法值
- 与 Phase 2 prompt（line 133）保持一致的枚举约束
- 确保独立的模型调用不会因缺少上下文而返回不合法值

## 向后兼容

- Prompt 变化不影响既有成功路径（Phase 3 本来就应该返回 `'direct'` 或 `'contextual'`）
- 只改进潜在失败路径（模型返回其他值时，现在有明确的枚举列表作为约束）
- 不改变 `StoryboardContent` schema、`StoryboardShot` 结构或公开命令

## 同步文档

- **docs/architecture.md**：在维护记录中新增 2026-08-18 条目，记录 Phase 3 prompt 改进
- **docs/api.md**：无需更新（prompt 内容不属于公开 API）
- **TASKS.md**：在当前任务窗口新增本项，标记为 Phase 3 prompt 改进

## 公开契约

无变更。

## 验证证据

- ✅ Rust 库测试：113 passed + 2 contract tests
- ✅ Rust fmt/check：通过
- ✅ harness:test：通过
- ⏳ 实际 storyboard 生成场景验证：需要用户测试 Phase 3 重试时，模型是否正确填写 `matchLevel` 字段

## 后续任务

1. **收集实际效果**：用户触发 Phase 3 重试时，观察模型是否始终返回 `'direct'` 或 `'contextual'`
2. **评估其他枚举字段**（如需要）：检查 `scriptMode` 等其他枚举字段是否在所有阶段都有明确约束
