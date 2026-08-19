# Storyboard 选镜优化系统

日期：2026-08-18

## 变更

### 第一阶段：暴露质量分数与多样性门

- `models.rs` 的 `StoryboardSource` 和 `SceneSegment` 新增 `visual_quality_score` 与 `scene_duration_ms` 字段（`Option` 类型，向后兼容旧记录）。
- `storyboard.rs::storyboard_sources` 从技术元数据提取首个视觉证据的质量分数，并为每个场景段计算 `scene_duration_ms = end_ms - start_ms`。
- `storyboard.rs::validate_storyboard` 新增 `validate_shot_diversity` 多样性硬门：连续镜头禁止使用同一素材；单一素材占比不得超过 40%。违反时给出可操作建议并拒绝候选。
- `storyboard.rs::storyboard_repair_message` 优化失败反馈，明确受影响的镜头索引。

### 第二阶段：独立评分子模块

- 提取 `storyboard/scoring.rs` 独立评分子模块（147 行）。
- `ScoredCandidate` 结构体封装评分结果。
- `rank_segment_candidates` 实现综合评分：
  - 语义相关性（0-50 分）：当前粗略用 visual_evidence/ocr 命中数估算
  - 画面质量（0-25 分）：来自 `visual_quality_score`
  - 时长匹配度（0-15 分）：候选时长与目标时长的适配度
  - 多样性惩罚（-10 分）：连续使用同一素材降权
  - 新鲜度（0-10 分）：根据项目内使用次数降权（TODO：从 DB 读取）
- `storyboard.rs::request_storyboard` 使用评分模块对候选排序，只向模型提供评分最高的前 5 个候选，提高选镜精度。
- 新增 4 个评分单元测试验证质量、时长、多样性和排序逻辑。

### 第三阶段：语义匹配层架构

- 新建 `storyboard/semantic.rs` 子模块架构（77 行）。
- `SceneEmbedding` 定义场景语义向量存储结构（time_range + 768/1024 维向量）。
- `encode_beat_semantics` 与 `search_by_semantic_similarity` 定义文本编码和相似度搜索接口。
- `cosine_similarity` 定义余弦相似度计算辅助函数。
- 当前阶段暂不实现具体 embedding 模型调用，保留 TODO 标记供后续集成 CLIP 文本/图像编码器。
- 新增 2 个占位测试验证接口可用性。

### 第四阶段：对抗验证框架

- 新建 `storyboard/validation.rs` 子模块架构（99 行）。
- `ValidationResult` 枚举定义验证结果：`Approved` 或 `NeedsRevision`。
- `RejectedBeat` 定义被拒绝的 beat 及拒绝原因。
- `verify_storyboard_selections` 定义对抗验证接口，让独立验证模型逐个审查已选镜头是否真的符合用户要求。
- 当前阶段返回 `Approved` 作为占位，实现体保留 TODO 标记供后续集成独立审查角色的模型验证循环。
- 新增 1 个占位测试验证接口可用性。

## 边界

- **向后兼容**：新增字段使用 `Option` 类型，旧记录安全读取为 `None` 或默认值。
- **公开契约不变**：Tauri 命令、SQLite schema、Agent 工具白名单均未改变。
- **架构解耦**：评分、语义、验证三个子模块各自独立，可单独演进和测试。
- **实现分阶**：第三、四阶段只定义接口和类型，实现体保留 TODO 并在独立任务中完成，避免半完成功能影响主线。

## 验证

- `cargo test --lib`：110 个测试通过
- `cargo fmt --check`：格式检查通过
- `npm run lint`：前端 lint 通过
- `npm run build`：前端构建通过
- `npm run harness:check`：架构与文档同步检查通过

同步文档：`docs/architecture.md`、`docs/api.md`、`TASKS.md`。
