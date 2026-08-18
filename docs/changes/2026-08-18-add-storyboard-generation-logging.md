# 2026-08-18: 为 storyboard 生成流程添加详细日志

## 变更类型

feature（诊断与调试支持）

## 触发规则

- desktop-contract（修改 `src-tauri/src/storyboard.rs`）

## 变更范围

### storyboard 生成日志（src-tauri/src/storyboard.rs）

**`generate_storyboard_internal` 函数（行 727-828）**
- 新增入口日志（行 727）：记录 `project_id`、`editing_task_id`、brief 长度、是否调度视觉分析
- 新增素材库存日志（行 747）：记录总素材数、视觉就绪数、视频/图片分类计数
- 新增每轮重试日志（行 772）：记录当前尝试次数 `revision + 1 / MAX_STORYBOARD_REVISIONS`
- 新增候选接收日志（行 796）：记录模型返回的 shots 数、beats 数、目标时长、未覆盖 beats 数
- 新增验证成功日志（行 808）：简短确认验证通过
- 新增最终失败日志（行 828）：记录尝试次数和最终反馈消息

**`request_storyboard` 函数（行 135-260）**
- 新增排序过程日志（行 135）：记录候选总数、目标时长、先前选择数量
- 保留既有候选清单日志（行 144）：记录 TOP-5 素材 ID、类型和时长
- 新增多模态内容构建日志（行 226-231）：记录候选数量和生成的 content blocks 数量
- 新增模型请求日志（行 238）：记录模型名称、content_parts 数量、超时设置
- 新增模型响应日志（行 260）：记录 JSON 响应长度

**`normalize_storyboard_candidate` 函数（行 545-607）**
- 新增归一化入口日志（行 547）：记录初始 shots 数和目标时长
- 新增视频范围修正日志（行 565）：记录每个被修正的 shot 的原始和修正后时间范围
- 新增脚本模式降级日志（行 595）：记录从 `full_script` 降级为 `key_message` 的原因
- 新增归一化完成日志（行 601）：记录修正次数、最终时长、脚本模式

## 日志级别

- `log::info!` - 正常流程关键决策点（入口、库存、排序、候选、归一化、成功）
- `log::warn!` - 预期但需要注意的情况（当前未添加，保留给网格生成失败等场景）
- `log::error!` - 失败路径（最终重试失败）

## 日志输出示例

```
[INFO] Starting AI storyboard generation. project_id=abc-123, editing_task_id=def-456, brief_length=248, schedule_visual_analysis=true
[INFO] Loaded storyboard sources: total_count=15, visual_ready_count=12, video_count=13, image_count=2
[INFO] Ranked 15 candidates for storyboard. target_duration_ms=30000, prior_selections=0
[INFO] Storyboard candidates (top 5 of 15 total): asset-1(video:45000ms), asset-2(video:30000ms), asset-3(video:60000ms), asset-4(image:0ms), asset-5(video:25000ms)
[INFO] Building multimodal content blocks for 5 candidates with keyframe grids
[INFO] Added 10 multimodal content blocks (image + metadata pairs)
[INFO] Sending storyboard request to model: model=gpt-5.4, content_parts=11, timeout=120s
[INFO] Received model response: json_length=3482 bytes
[INFO] Storyboard generation attempt 1/3
[INFO] Received storyboard candidate: shots=8, beats=5, target_duration_ms=28000, uncovered_beats=0
[INFO] Normalizing storyboard candidate: shots=8, initial_target_duration_ms=28000
[INFO] Corrected video range for shot_0: asset_id=asset-1, [5000-9000]ms -> [4800-8800]ms
[INFO] Corrected video range for shot_3: asset_id=asset-2, [12000-16000]ms -> [11500-15500]ms
[INFO] Normalization complete: corrections=2, final_duration=28000ms, script_mode=key_message
[INFO] Storyboard validation passed.
```

失败场景示例：
```
[ERROR] Storyboard generation failed after 3 attempts. final_feedback=Some("Asset 'asset-1' appears in 4 of 8 shots (50%), exceeding the 40% diversity limit...")
```

## 同步文档

- **docs/architecture.md**：已更新素材分析流程说明，明确 storyboard 生成会记录详细日志（入口参数、素材库存、重试进度、验证结果等）
- **docs/api.md**：无需更新（日志为内部诊断工具，不影响公开 API）
- **README.md**：无需更新（日志输出不影响用户可见功能）
- **docs/decisions.md**：无需更新（日志实现不涉及架构决策）
- **TASKS.md**：已更新当前任务窗口，标记日志任务完成

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
- ✅ harness:test：通过
- ⏳ harness:check：需要同步 docs/architecture.md 后通过

## 日志覆盖范围

已覆盖：
- ✅ 生成入口参数（project_id、editing_task_id、brief 长度）
- ✅ 素材库存统计（总数、视觉就绪数、视频/图片计数）
- ✅ 候选排序过程（输入参数、输出 TOP-5 清单）
- ✅ 多模态内容构建（候选数、blocks 数）
- ✅ 模型请求/响应（模型名、content_parts 数、JSON 长度）
- ✅ 重试循环进度（当前轮次）
- ✅ 候选接收（shots、beats、时长、未覆盖 beats）
- ✅ 归一化过程（修正次数、脚本模式降级）
- ✅ 验证结果（成功/失败）
- ✅ 最终失败总结（尝试次数、反馈消息）

未覆盖（未来可补充）：
- ⏸️ 各个验证函数的具体失败原因（`validate_non_overlapping_video_sources`、`validate_shot_diversity`）
- ⏸️ 网格图 base64 编码进度（`multimodal::build_multimodal_content` 内部）
- ⏸️ 模型请求耗时（需要计时器支持）

## 后续任务

1. **测试日志输出**：在实际 storyboard 生成流程中验证日志完整性和可读性
2. **调整日志级别**：根据生产环境反馈调整 info/warn/error 的使用
3. **补充验证函数日志**（可选）：在 `validate_non_overlapping_video_sources` 等函数中记录具体失败的 shot 对
4. **补充性能日志**（可选）：记录关键阶段耗时（排序、归一化、验证）

## 风险与限制

- **日志量增长**：每次 storyboard 生成会产生 10-15 条 info 日志，多次重试会成倍增长
- **敏感信息泄露风险**：当前日志不包含文件路径、模型原文或媒体证据，但包含 project_id、editing_task_id 和 asset_id（UUID 形式，无直接敏感性）
- **日志格式稳定性**：日志消息为纯文本，未来解析需要正则表达式或结构化日志支持
- **测试覆盖**：当前未添加日志输出的单元测试（Rust `log` crate 需要 test fixture 捕获输出）
