# 选镜 / 策划评测基线

任务日期 2026-10-01；正式复跑于 2026-10-04 完成。本分支基于 `4c68ad2`，模型运行时保存了 dirty diff 与含新增文件的源码树哈希，结果不是以 HEAD 冒充干净工作区。

## 交付行为

新增 `scripts/footage-eval/` 的冻结、真实运行、计分和 CSV 纠正入口。无窗口 Rust 二进制使用现有 Agent 工具循环、策划/选镜、时间线及 FFmpeg 预览链路。固定 10 例各 3 次；一条命令从冻结快照建独立数据库和空产物目录，输出报告、阶段轨迹、候选/入池淘汰理由、计划、时间线、音频时钟、产物路径与回执。详见 `docs/evaluation/README.md`、`docs/evaluation/baseline-2026-10-01.md`。

只读 SQLite backup 找回 09-27 三次原始需求和开关。原项目的 94 个工业视频 ID 已移除，按同一源路径绑定当前活跃的 94 个 ID 与分析；保存绑定表、原片内容哈希、分析/向量/帧快照和历史使用次数。共享库原本可访问 665 条素材，本评测限定为指定 94 条工业视频 + 原有音频；每例使用次数从 0 开始。因此是“历史输入复现 + 当前冻结分析”，不是 09-27 全环境复刻。

输出在 `.footage-eval/`，不进入 Git。金标入口包含 141 个真实片段的 `gold.csv`，以及正式结果的 30 行 `run-review.csv`、132 行 `selection-review.csv`。风险、品牌、最佳范围、语义支持与体裁提示均明确是机器预标，待用户纠正；未知不能获得 0 风险或满分。

## 生产文件里的加法与隔离约束

默认桌面构建不包含 `footage-eval`，没有修改任何策划、选镜、排序、源窗或时长算法。

| 文件 | 改动及边界 |
|---|---|
| `src-tauri/Cargo.toml` | 显式 feature 与仅该 feature 可用的二进制；default-run 钉回现有桌面主二进制，避免额外 CLI 改变默认入口。 |
| `src-tauri/src/lib.rs` | 仅 feature 导出评测入口；没有增加/修改 Tauri IPC 命令或前端桥。 |
| `src-tauri/src/agent.rs` | 将既有 pipeline 的可见性扩至 crate 内，参数和执行行为不变。 |
| `src-tauri/src/storyboard/phases.rs` | 仅 feature 记录 Phase 1 原始输入/响应、P2 排序库存和实际入池尝试原因，不重排、不增加判断。 |
| `src-tauri/src/storyboard/provider_trace.rs` | 评测时轨迹改写到该次输出；评测 feature 下用 mutex 防止并发正文与换行交叉追加。 |
| `src-tauri/src/agentloop/trace.rs` | 评测时导出到该次输出并遮蔽敏感字段/图片，模型收到的输入不变。 |
| `src-tauri/src/provider.rs` | 评测激活时仅只读自定义模型凭据，不刷新 OAuth/网关登录令牌，也不切换 Provider；正常桌面路径不变。 |
| `src-tauri/src/agentloop/native.rs` | 评测激活时拒绝重新分析及直接剪映交付工具，防止冻结分析被重跑、写入外部草稿库。 |
| `src-tauri/src/handoff/deliver.rs` | 评测激活时外部编辑器交付返回明确受限原因；已生成故事版、时间线和预览不改。 |
| `src-tauri/src/cards/renderer.rs` | 评测激活时拒绝卡片 WebView，预览检查保存缺失原因；不启动 GUI。 |

后四项是评测专用安全边界，故本基线不能证明真实网关、品牌卡渲染或编辑器交付成功。已有项目品牌文案“智造未来”照录，不作为画面标识归属的金标。

## 实际基线与限制

正式命令：

```powershell
python scripts/footage-eval/run.py --snapshot .footage-eval/frozen-2026-10-01 --output .footage-eval/baseline-final-2026-10-01 --workers 6
```

命令正常结束，30 次都有真实模型结果。Provider 为自定义 Agnes `agnes-3.0-flash`；英文固定旁白的实际 TTS 为 Fish Audio `s2.1-pro-free`（原有配置，本任务未切换服务）。局部换镜实际改变第一个镜头 1/3：另一次 completed 却未修改、一次 Provider 网络失败；失败时保留 setup 产物，不能把它算成成功换镜。所有运行的窗口计数为 0。

策划实际仍是无素材引用的文字阶段：有 Phase 1 的策划引用覆盖率为 0%。有成片的最终拍覆盖 100%，重复 asset、重叠源窗和时长偏差为 0；但这仅是结构约束，不证明语义直证或“最好区间”。宣传选镜有 1–6 条风险预标；数次选中区间跨已冻结硬切边界。生成请求也可能以 completed 结束却只给“先看看”或待确认方案，无成片。英文旁白 3 次均因实际约 9.4–9.7 秒与 15 秒目标冲突而暂停。完整均值/最差值与测量次数见基线文档。

局部换镜的一次回复还把整段内部媒体/工具指令输出给用户，且搜索失败后说“我会重试”就结束，没有实际更换。这是任务 7 的范围外问题，原始回复已保留；本任务不修改 Agent 收尾行为。

10-01 初次运行发现观测 JSONL 并发粘连，导致一例汇总异常；保留原始证据，用完整 JSON 对象读取恢复，再以修正后的轨迹写入器实际重跑正式全套。响应回放未用作质量评测。

重新导入分析未实现，仍是单独轨道；人工金标、三体裁真实素材与用户看片尚缺。无必要问题与完整自然语言事实一致性需要人工纠正，自动结果只覆盖澄清/确认预标、回执 ID/版本/预览文件存在性及 completed 无产物的固定事实。未给这些缺金标项目假分数。

## 检查与主会话衔接

`cargo check --manifest-path src-tauri/Cargo.toml`、评测 feature 编译/全套真实运行、7 项计分 fixture 检查、只重计分、`git diff --check` 均执行。未改前端，不需要 lint。`npm run harness:check` 的架构、Agent 可信边界检查通过；文档同步规则因 `src-tauri/src/lib.rs` 被修改而机械要求同步 `docs/api.md`。

本任务明确禁止修改 `docs/api.md`（由另一个执行会话拥有），因此没有触碰它，也没有伪造检查通过。这里记录供主会话衔接：新增的是 feature CLI，没有 IPC 变化；由该文档所有者补上说明或对规则作有依据的处理后，再按实际合并版本复查 harness。未改 `TASKS.md`、`docs/decisions.md`、方案、架构/README 或其他会话拥有的脚本。
