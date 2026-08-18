# 2026-08-18: 重构 storyboard 生成为三阶段架构

## 变更类型

refactor（架构重构）

## 触发规则

- desktop-contract（修改 `src-tauri/src/storyboard.rs` 和新增 `src-tauri/src/storyboard/phases.rs`）

## 问题背景

**原有实现的根本缺陷：**
- 对整个素材池（426 个视频）做**一次全局排序**，取 TOP-5 候选
- 把这 5 个素材交给模型，让模型从中组合出整个 storyboard（8 个 shots）
- **结果：整个时间线只能从同一组 5 个素材中反复选择**

这相当于"整条时间线只能从 5 个预选素材中拼凑"，导致：
1. 素材多样性严重受限（426 个视频只用 5 个）
2. 模型无法为不同叙事节奏选择最优素材
3. 验证失败时重试仍在同一组 5 个素材内打转

## 变更范围

### 新增三阶段流程模块（src-tauri/src/storyboard/phases.rs）

**Phase 1: 叙事结构生成**
```rust
pub(crate) fn phase1_generate_narrative(
    access: &ModelAccess,
    brief: &str,
) -> Result<NarrativeStructure, String>
```
- 输入：brief
- 输出：`NarrativeStructure`（title, summary, targetDurationMs, scriptMode, beats）
- 模型根据内容的自然节奏、节奏要求和叙事复杂度确定合适的 beats 数量。简单消息可能只需 3-4 个 beats，故事驱动的内容可能使用 8-12 个或更多。让内容引导结构，不人为限制或填充 beat 数量。
- 每个 beat 包含：
  - `id`：唯一标识
  - `purpose`：该 beat 的叙事作用
  - `requiredVisual`：该 beat 需要的视觉证据要求
- **不涉及素材选择**，纯粹的故事结构设计

**Phase 2: 逐 beat 粗选镜**
```rust
pub(crate) fn phase2_rough_shot_selection(
    access: &ModelAccess,
    brief: &str,
    narrative: &NarrativeStructure,
    sources: &[StoryboardSource],
) -> Result<RoughStoryboard, String>
```
- 对**每个 beat**：
  - 根据 `requiredVisual` 单独对素材池排序（使用 `scoring::rank_segment_candidates`）
  - 提供该 beat **专属的 TOP-5 候选素材**（带关键帧网格）
  - 模型为该 beat 选择 1 个素材 + 时间范围
- 输出：`RoughStoryboard`（每个 beat 一个 shot，可能时长不精确）
- 日志记录每个 beat 的专属 TOP-5 清单

**Phase 3: 精剪与节奏优化**
```rust
pub(crate) fn phase3_fine_edit(
    access: &ModelAccess,
    brief: &str,
    rough: &RoughStoryboard,
    sources: &[StoryboardSource],
    feedback: Option<&str>,
) -> Result<StoryboardContent, String>
```
- 输入：Phase 2 的粗略 storyboard + 验证反馈（如有）
- 模型调整：
  - 精确时间范围（对齐场景边界 `scene_segments`、避免重叠）
  - 节奏控制（调整每个 shot 时长以匹配整体节奏）
  - 镜头组合（某些 beat 可能需要拆分成多个 shots）
  - 过渡优化（确保相邻 shots 的视觉连贯性）
- 输出：最终可执行的 `StoryboardContent`
- **重试循环只在 Phase 3**：验证失败时带反馈重新精剪，最多 3 次

### 主流程重构（src-tauri/src/storyboard.rs:878-949）

**原有流程：**
```rust
for revision in 0..MAX_STORYBOARD_REVISIONS {
    match request_storyboard(&access, brief, &sources, previous, feedback) {
        // 全局 TOP-5 候选 + 模型一次性生成完整 storyboard
    }
}
```

**新流程：**
```rust
// Phase 1: 生成叙事结构（不重试）
let narrative = phases::phase1_generate_narrative(&access, brief)?;

// Phase 2: 逐 beat 粗选镜（不重试）
let rough = phases::phase2_rough_shot_selection(&access, brief, &narrative, &sources)?;

// Phase 3: 精剪与验证重试循环
for revision in 0..MAX_STORYBOARD_REVISIONS {
    match phases::phase3_fine_edit(&access, brief, &rough, &sources, feedback) {
        // 验证失败时只重新精剪，Phase 1/2 结果保留
    }
}
```

### 数据结构（src-tauri/src/storyboard/phases.rs:14-36）

```rust
/// Phase 1 输出：纯叙事结构
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NarrativeStructure {
    pub title: String,
    pub summary: String,
    pub target_duration_ms: i64,
    pub script_mode: String,
    pub beats: Vec<StoryboardBeat>,
}

/// Phase 2 输出：粗略 storyboard（每个 beat 一个 shot）
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoughStoryboard {
    pub title: String,
    pub summary: String,
    pub target_duration_ms: i64,
    pub script_mode: String,
    pub beats: Vec<StoryboardBeat>,
    pub uncovered_beat_ids: Vec<String>,
    pub shots: Vec<StoryboardShot>,
}
```

## 架构优势

1. **素材多样性提升**：每个 beat 都有独立的 TOP-5 候选池，不再受全局 5 个素材限制
2. **语义匹配精度提升**：排序算法针对每个 beat 的 `requiredVisual` 计算，而非全局平均
3. **重试效率提升**：Phase 3 验证失败时只重新精剪，Phase 1/2 的叙事结构和候选池保持稳定
4. **调试透明度提升**：日志清晰展示每个 beat 的专属候选清单，易于诊断选择偏差
5. **模型负载分散**：三次小请求（叙事、粗选、精剪）替代一次大请求，降低超时风险

## 向后兼容

- Tauri 命令 `generate_storyboard`/`generate_storyboard_for_agent` 签名不变
- SQLite schema 不变（`storyboard_versions` 表结构不变）
- 最终持久化的 `StoryboardVersion` 结构不变
- 旧的 `request_storyboard` 函数保留但标记为 `#[allow(dead_code)]`，未来清理

## 日志输出示例

```
[INFO] Starting AI storyboard generation. project_id=abc-123, editing_task_id=def-456, brief_length=248, schedule_visual_analysis=true
[INFO] Loaded storyboard sources: total_count=426, visual_ready_count=426, video_count=426, image_count=0, audio_count=0, other_count=0
[INFO] Sample of loaded sources (first 10): asset-1(video:45000ms), asset-2(video:30000ms), ...
[INFO] Phase 1: Generating narrative structure from brief
[INFO] Phase 1 complete: received narrative structure, json_length=1234 bytes
[INFO] Phase 1 complete: narrative with 8 beats, target_duration=30000ms
[INFO] Phase 2: Rough shot selection for 8 beats
[INFO] Beat 'opening': ranked 426 candidates, top 5: asset-12(video), asset-34(video), asset-56(video), asset-78(video), asset-90(video)
[INFO] Beat 'problem': ranked 426 candidates, top 5: asset-45(video), asset-67(video), asset-89(video), asset-21(video), asset-33(video)
... (每个 beat 的专属 TOP-5 清单)
[INFO] Phase 2 complete: received 8 shots, json_length=5678 bytes
[INFO] Phase 2 complete: rough storyboard with 8 shots, 0 uncovered beats
[INFO] Phase 3 attempt 1/3: fine editing with validation
[INFO] Phase 3 produced candidate: shots=8, beats=8, target_duration_ms=30000, uncovered_beats=0
[INFO] Normalizing storyboard candidate: shots=8, initial_target_duration_ms=30000
[INFO] Normalization complete: corrections=2, final_duration=30000ms, script_mode=key_message
[INFO] Storyboard validation passed.
[INFO] Storyboard content finalized. Persisting to database.
```

## 同步文档

- **docs/architecture.md**：需更新素材选择流程说明，从"全局 TOP-5"改为"逐 beat 专属 TOP-5"
- **docs/api.md**：无需更新（Tauri 命令签名不变）
- **TASKS.md**：需更新当前任务窗口，标记三阶段重构完成
- **docs/decisions.md**：无需更新（三阶段流程是实现细节，不涉及公开 ADR）

## 公开契约

### Tauri 命令

无变更。

### SQLite schema

无变更。

### Agent 工具

无变更。

## 验证证据

- ✅ Rust 库测试：113 passed
- ✅ Rust fmt/check：通过
- ✅ 前端 lint/build：通过
- ✅ harness:test：通过
- ⏳ harness:check：需要同步 docs/architecture.md、docs/api.md、TASKS.md 后通过

## 风险与限制

- **Phase 1/2 失败无重试**：如果 Phase 1 或 Phase 2 的模型请求失败，整个 storyboard 生成直接失败（不进入重试循环）。理由：叙事结构和粗选结果应该是稳定的，重试应该只针对 Phase 3 的精剪调整。
- **Phase 2 的 TOP-5 不持久化**：当前实现不保存每个 beat 的候选清单，Phase 3 重试时无法看到 Phase 2 的原始候选。未来可考虑在日志或中间结果中持久化候选清单。
- **排序算法仍使用现有 `scoring` 模块**：Phase 2 的逐 beat 排序仍使用 `rank_segment_candidates`，未来可针对 `requiredVisual` 实现更精细的语义匹配（如 embedding 相似度）。
- **Phase 3 的 prompt 变长**：Phase 3 需要传递完整的 `RoughStoryboard` 和所有 `sources`（含 `scene_segments`），prompt 长度显著增加。未来可考虑只传递粗选 storyboard 中已使用的素材。

## 后续任务

1. **测试三阶段流程**：在实际 storyboard 生成中验证每个 beat 的专属 TOP-5 是否有效提升多样性
2. **优化 Phase 3 prompt**：只传递粗选 storyboard 中已使用的素材，减少 context 长度
3. **持久化中间结果**（可选）：保存 Phase 1 的 `NarrativeStructure` 和 Phase 2 的 `RoughStoryboard`，支持断点续传或手动干预
4. **语义匹配层集成**（可选）：在 Phase 2 中使用 embedding 相似度替代当前的 `rank_segment_candidates`
5. **清理旧代码**：移除 `request_storyboard` 函数和相关的全局 TOP-5 逻辑
