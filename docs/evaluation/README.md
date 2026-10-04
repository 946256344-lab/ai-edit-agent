# 选镜 / 策划评测

本运行器观察现有 Agent → 策划 → 选镜 → 时间线 → 预览，调用真实 Provider，不修改生成算法。每例从同一冻结快照的空产物状态开始，项目历史使用次数留档，评测初始使用次数统一为 0。当前只跑 `frozen_existing_analysis`；重新导入分析是单独轨道，尚未实现，不混进本基线。

## 一条命令

在独立 worktree 根执行（Windows、Python 3、Rust 工具链、已配置自定义模型 Provider）：

```powershell
python scripts/footage-eval/run.py --workers 6
```

默认在 `.footage-eval/<时间戳>/` 新建输出，自动只读备份本机 SQLite 一次，查找原有 94 条工业视频项目，冻结原片 SHA256、当前活跃分析、向量、帧及本地模型权重，然后构建无窗口评测二进制并跑 10 例 × 3 次。默认不传 `--workers` 时全部 30 个独立进程并行；16 GB 内存机器建议显式用 6，模型链路内的并发与 429 退避沿用生产行为。`--source-data <绝对路径>`、`--project <ID>` 可指定来源；来源只读。

worktree 没有依赖时，仅建立用户授权的目录链接，不运行 npm install：

```powershell
cmd /c mklink /J node_modules D:\自动剪辑系统\ai-edit-agent\node_modules
```

开发媒体运行时需位于本 worktree 的 `src-tauri/resources/ffmpeg/`，DirectML 在 `src-tauri/resources/directml/`；也可用现有媒体工具环境配置。运行器不安装或更新真实应用。模型权重从应用快照复制，运行中不重新导入原片或开启后台视觉分析。

复跑同一冻结输入：

```powershell
python scripts/footage-eval/run.py --snapshot .footage-eval/frozen-2026-10-01 --output .footage-eval/rerun-2026-10-01 --workers 6
```

输出目录必须是新的，避免混入上次产物。`--skip-build` 仅在已确认二进制与源码一致时使用。报告有 HEAD、dirty diff 哈希和含新增源文件的源码树哈希；冻结快照保存数据库、分析、向量和素材清单哈希，每次复跑核对原片内容未变化。

## 隔离边界

- 原始 SQLite 使用 `mode=ro` 的 online backup，包含已提交 WAL，不执行真实库迁移或恢复。之后只查询和修改输出内的副本。
- 旧项目的 94 个原有 ID 可能已移除并重新导入：按完全相同的源路径绑定当前活跃 ID 与分析，保存 `inventory-bindings.json`，不靠文件名判断内容。不存在活跃分析时保留旧记录的真实状态，不伪造 ready。
- 测试范围固定为这 94 条工业视频 + 原项目已有音频。共享库额外素材不参与；每次清空所有计划、时间线、任务、会话与使用历史。
- Python 运行器在创建输出前拒绝真实应用数据目录及其子目录；Rust 入口另行核对默认真实目录。Tauri Context 清空窗口配置，再用绝对输出数据根作为 identifier；使用数据库前断言所有 app data/cache/config/log 目录都在输出内。TEMP/TMP 也指向该次输出。不调用桌面 `run()`，不启动开发服务器。
- 原媒体只读，分析帧在输出内复制一次。每次模型权重从输出快照建硬链接，现有调用只读权重；所有新生成缓存、音频和预览进入各自 appdata。
- 评测 feature 激活时不刷新 OAuth / 网关登录凭据，仅使用当前自定义 Provider 的系统凭据；无配置就如实失败，绝不切换 Provider。网关/OAuth 路径暂未实现安全只读评测。
- 禁止外部编辑器交付、卡片 WebView，以及重新分析工具。生成仍可得到真实故事版、时间线、配音/BGM 和视频预览；这些限制会在工具回执/预览检查里报出，不能用本评测证明编辑器或品牌卡可交付。
- 初始项目设置照录照用；“智造未来”等已有品牌文案不等同于用户确认的允许品牌身份。

## 固定用例与来源

`scripts/footage-eval/cases.json` 固定 10 例：历史 3 例、中英文宣传、15/30 秒、配音/BGM 开关、缺画面、局部换镜、叙事缺因果链、工业花絮。

优先按历史 `userMessageId` 在备份中恢复请求及 task 媒体快照；未找到时使用明确标注的“新基线，非历史复现”。找到时也只称“历史输入复现”，当前分析与代码不是 09-27 环境完整复刻。后续同名库被重新导入的影响在绑定表里保留。

局部换镜例先从空状态生成 15 秒草稿，再在同一隔离库提交换镜请求；其余镜头和音轨与 setup 时间线逐字段对账；必须 completed 且目标镜头实际改变才算局部换镜成功，保留 setup 产物不算成功。setup 失败时标局部操作未执行，不能算冻结约束通过。

## 指标定义

每个指标报告均值、最差值和实际测量次数。覆盖率越低越差，计数/偏差越高越差。N/A 不补 0；有足够素材却未产出必须单独显示，不能因空结果获得满分。

| 指标 | 自动口径 / 限制 |
|---|---|
| 策划引用覆盖 | Phase 1 返回的每拍是否已有合法素材 ID；现行纯文字策划不含引用就为 0，缺 Phase 1 为 N/A。不得用后续选镜覆盖替代。 |
| 最终拍覆盖 | 最终故事版拍数为分母；shot 的 beatId 对应拍、asset 存在且原片可访问、源区间合法且位于所选候选窗内，才覆盖该拍。当前链路允许 `s001+s002` 的相邻合并窗，按其冻结范围核对；另报非法 ID/不可访问、越窗数以及跨硬切片段数，不把跨硬切混成非法 ID。 |
| 素材复用/重叠 | 最终时间线 source clips 为准；相同 assetId 超出第一次的次数；同资产任意两源区间重叠毫秒求和。 |
| 相似复用代理 | 同一 sceneSegment 的重复次数，仅是确定性代理。跨资产的画面相似尚缺金标，不能把代理 0 当成无相似画面。 |
| 时长/时钟 | 最终 source clips 最大 timelineEndMs 对目标的绝对 ms 与百分比偏差；主时钟为启用 voiceover cue 结束，否则 music cue 结束，否则画面结束。报告音轨数量与区间、画面空洞 ms、最终源窗越界数。精确时长目标门槛建议 max(一帧,2%)，目前基线只度量，不改变时长算法。 |
| 澄清/问题 | 自动统计 needs_clarification 的轮数和回复中的问题/确认提示预标。自然语言必要性未判金标；状态 completed 也可能询问用户，不能只看状态。 |
| 回执产物事实 | 返回的故事版/时间线 ID 与 task 版本号对 DB、预览路径是否真实存在。不扩张成全部自然语言事实通过。 |
| 请求目标与状态 | 生成请求返回 completed 但无任何 source clip，记 1；失败/澄清与部分产物单独展示。缺画面例仍需人工判断是否诚实说明而非硬凑。 |
| 机器风险预标 | 按已保存画面识别的 brandLogos、exhibition、focus、qualityNotes/caption/scene 的正向风险词，与最终源窗重叠计数。未知不视为安全；不是硬淘汰金标分数，不得用于宣称识别完成。 |
| 硬淘汰金标 | 全部选中片段均被人工确认相应体裁风险时才给全片计数，否则 N/A。另报金标覆盖率。 |
| 最佳窗金标 | 用户填可接受最佳范围后检查选中窗是否包含在其内；未标不称“最好”。 |
| 语义直证/体裁/关系/素材足够 | 机器概念、候选 matchLevel 和体裁提示留作预标。没有人工金标不输出正确率、漏检/误杀率或“足够素材”的结论。当前链路没有结构化体裁字段，手选只以固定需求文字表达，不冒充新 UI 体裁入口已实现。 |

## 金标纠正

`snapshot/gold.csv` 每个真实场景片段一行（UTF-8 BOM，Excel 可打开），有 asset ID、源窗、识别原文、可能品牌/展会/失焦/抖动等风险、帧路径、机器 bestRange 与概念提示。正向命中也只是“预标，待用户纠正”；风险词可能包含否定或模型误识别，用户看片后决定。

仅填人工列，保留机器列用于比较：

- `confirmed=yes`；`genre=promotion/narrative/bts`。同片多体裁标注可复制一行，历史自动宣传例按 promotion 计分。
- `gold_risks=none` 表示已看过且无该体裁风险；否则填原因，用 `|` 分隔。`risk_start_ms/risk_end_ms` 填风险时段；不填则保守按整片段风险，不凭空猜时段。
- `brand_identity` 填实际品牌，并说明是否为本项目允许品牌。仅填写项目设置名称不足以证明画面中的标识属于本项目。
- `best_start_ms/best_end_ms` 填可接受源窗；`supports` 填能直证的卖点/事件描述；`split=calibration/holdout` 固定样本划分，别把调参样本当保留验收样本。
- `notes` 可说明镜头关系、动作完整性、素材是否足够。语义/关系/体裁的文字标注目前用于人工审阅，运行器不会把自由文本自动变成假正确率。

只重计分，不调用模型：

```powershell
python scripts/footage-eval/run.py --rescore --output .footage-eval/live-baseline-2026-10-01 --gold .footage-eval/frozen-2026-10-01/gold.csv
```

保留原始 gold.csv 和纠正版本方便对比。缺标样本保持 N/A。本任务没有人工金标，因此不能给语义直证、最好区间、体裁或硬风险的正式分数。

另有两份针对实际输出的纠正表：`run-review.csv` 中填写无必要问题数、回复事实不一致数（整数，0 也须明确填写）、`gold_genre_correct=yes/no` 与素材是否足够；`selection-review.csv` 每个最终镜头填写 `gold_hard_risk`、`gold_direct_support`、`gold_best_window`、`gold_relation_violation` 的 yes/no。重新计分会读取两表，覆盖率未满仍保留 N/A；表内人工列不会在重算时被覆盖。`supports` 自由文本只是备注，选镜表的明确 yes/no 才参与语义覆盖计算。

## 输出和检查

`experiment.json`、`cases.json`、`summary.json`、`report.md` 是总入口。每次运行有 `initial-state.json`、`job.json`、Provider 元数据（无 key）、过程日志、native/storyboard JSONL、`result.json`、`evidence.json`、`metrics.json`；其中包含各阶段输入输出、排序候选和实际入池/淘汰原因、计划、时间线、音轨与回执。局部换镜另存 setup 证据。池满而未尝试的候选只记排名/未入池，不伪造逐条淘汰判断。

JSONL 可用于响应回放的规则检查；本运行器不提供模型响应替身，也不以回放替代真实模型质量评测。大文件与私人媒体路径只留 `.footage-eval/`，仓库只提交汇总报告。

```powershell
python scripts/footage-eval/test_metrics.py
cargo check --manifest-path src-tauri/Cargo.toml
cargo check --manifest-path src-tauri/Cargo.toml --features footage-eval --bin footage-eval
npm run harness:check
git diff --check
```

未改前端，不要求 lint。评测新 fixture 的计分测试验证空产物不能得满分、策划引用与最终选镜分开、越窗不算覆盖、音频 cue 时钟、粘连轨迹恢复、风险/最佳金标独立覆盖及最差值方向，共 7 项，不铺生产算法测试。


## 证据契约评测与冻结规则回放（任务 2）

真实生成运行结束后会用同一 Rust 适配器从运行副本只读导出 `segment-evidence.json`。每例 `metrics.json.evidenceContract` 和报告追加逐片风险三态、来源、非未知风险覆盖率、未知比例（全部 11 项和核心 7 项）、旧机器正向预标覆盖率。分母是片段×风险，不是图片或最终入选数；旧预标包含宽泛关键词，覆盖率是两种预标的对照，不能冒充召回率。

仅重跑冻结分析适配、历史产物计分与现行 P5 校验（输出必须为新目录）：

```powershell
python scripts/footage-eval/run.py --contract-replay D:\自动剪辑系统\worktrees\footage-eval\.footage-eval\baseline-final-2026-10-01 --snapshot D:\自动剪辑系统\worktrees\footage-eval\.footage-eval\frozen-2026-10-01 --output .footage-eval/contract-replay-2026-10-05
```

输出 `contract-replay-report.json`、两次字节相同的契约导出、30 例旧核心指标比较、各例 `p5-replay.json`，并核对冻结数据库/分析/gold 哈希。只读源 worktree，不修改其中任何文件。`liveGeneration=false` 明确这是保存产物的规则回放；产物与指标不变不能证明真实模型重新生成仍选同一组镜头。真实模型全套复跑仍使用原 `--snapshot` 命令。

原始风险事实金标单独用 CSV：`asset_id,segment_id,risk,state,start_ms,end_ms`，`risk` 用 API 中 11 项名称，`state` 只填 `hit/not_hit`；时段为空按整片段。回放生成空白 `evidence-gold-template.csv`，填完传 `--evidence-gold <CSV>`，或放在输出快照 `evidence-gold.csv`。不写源快照。体裁 `gold_risks=none` 只说明对应体裁没有硬风险，不能用作所有原始风险的阴性金标。

指标分别报告金标阳性漏检（未知也记未检出，并单列未知数）与金标阴性误报率；误报是后续误杀风险的代理，本任务没有淘汰逻辑，不能声称测了真正的体裁误杀率。没有明确金标或相应分母为零就报 N/A；未知阴性样本不计为已证实 true negative。

补核验的隔离评测可创建 `job.json`，字段为 `mode=verify-evidence`、`projectId` 和 `candidates[]`（见 API 的 `EvidenceVerificationRequest`），把冻结 SQLite 复制到同目录的 `appdata/assembly-video-agent.sqlite3` 后调用 `src-tauri/target/debug/footage-eval.exe <job.json>`。不会创建窗口、调用生成链或修改原分析，只在该次副本追加核验表与 cache。结果在 `verification-result.json`，真实模型身份在 `provider.json`，均不含凭据。相同输入再次执行时已知项不再发请求；缺帧、错项目或过期快照均如实报错。
