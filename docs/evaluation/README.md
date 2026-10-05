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

## 只跑盘点、体裁、配方与引用策划（任务 4）

```powershell
python scripts/footage-eval/run.py --planning-only --snapshot D:\自动剪辑系统\worktrees\footage-eval\.footage-eval\frozen-2026-10-01 --output .footage-eval/planning-2026-10-05 --workers 4
```

使用真实 Provider，无 GUI、开发服务器、Agent、配音/BGM、时间线、预览或编辑器；10 个基线 setup 请求加自动叙事降级/信息不足默认各 3 次。局部换镜仅测 setup 策划，声音开关不在本模式合成。进程 workers 限 1–4；进程内独立模型请求按需并发，每请求 0 张图。只读冻结源，SQLite 导出在本输出副本打开，结果带源码树/二进制哈希、模型身份、逐请求轨迹、逐片盘点、体裁快照、配方、策划与机器指标。

可用 `--eligible-evidence <SegmentEvidence数组.json>` 提供任务 3 已补核验的冻结集合，核对 ID、分析快照和源窗。任务 4b 起，所有输入按实际体裁重新 `evaluate`；未提供补核验快照时旧未知不能放行。主接口始终要求调用方交合格集合；评测轨道标签不能进入生产放行依据。任务 4 历史报告的 `semantic_only_unqualified_frozen_input_not_safety_acceptance` 属旧评测方式。

报告每段合法主选引用覆盖（目标 100%，对比旧基线 0%）、非法引用/越窗（目标 0）、无策划次数、手选遵守、体裁及理由、缺口与每条拒绝原因（含被拒旧提案 `rejectedAttempts`，校正后新提案也要重新验收）。空产物覆盖率 N/A；缺画面/叙事缺因果的 `gap_only` 是合法缺口输出，不计为策划成功。缺口诚实性与语义/体裁正确率无人工金标保持 N/A，机器缺口预标单列。冻结文件哈希和每次窗口数保存在 `isolation-audit.json`。

原始风险事实金标单独用 CSV：`asset_id,segment_id,risk,state,start_ms,end_ms`，`risk` 用 API 中 11 项名称，`state` 只填 `hit/not_hit`；时段为空按整片段。回放生成空白 `evidence-gold-template.csv`，填完传 `--evidence-gold <CSV>`，或放在输出快照 `evidence-gold.csv`。不写源快照。体裁 `gold_risks=none` 只说明对应体裁没有硬风险，不能用作所有原始风险的阴性金标。

指标分别报告金标阳性漏检（未知也记未检出，并单列未知数）与金标阴性误报率；误报是后续误杀风险的代理，本任务没有淘汰逻辑，不能声称测了真正的体裁误杀率。没有明确金标或相应分母为零就报 N/A；未知阴性样本不计为已证实 true negative。

补核验的隔离评测可创建 `job.json`，字段为 `mode=verify-evidence`、`projectId` 和 `candidates[]`（见 API 的 `EvidenceVerificationRequest`），把冻结 SQLite 复制到同目录的 `appdata/assembly-video-agent.sqlite3` 后调用 `src-tauri/target/debug/footage-eval.exe <job.json>`。不会创建窗口、调用生成链或修改原分析，只在该次副本追加核验表与 cache。结果在 `verification-result.json`，真实模型身份在 `provider.json`，均不含凭据。相同输入再次执行时已知项不再发请求；缺帧、错项目或过期快照均如实报错。

## 体裁底线与入池对比（任务 3）

2026-10-05 的 10 例 × 3 次真实复跑及同片三体裁裁决结果见 [体裁底线评测](2026-10-05-genre-eligibility.md)，含未进入底线阶段、旧版局部换镜与缺金标的限制。

新生成 Phase 2 暂按宣传执行，先补核验召回所需候选的关键未知，再过代码硬门。按同一冻结快照真实复跑，显式限制进程并发为 4：

```powershell
python scripts/footage-eval/run.py --snapshot D:\自动剪辑系统\worktrees\footage-eval\.footage-eval\frozen-2026-10-01 --output .footage-eval/genre-eligibility-live-2026-10-05 --workers 4
src-tauri/target/debug/footage-eval.exe --export-evidence D:\自动剪辑系统\worktrees\footage-eval\.footage-eval\frozen-2026-10-01/assembly-video-agent.sqlite3 .footage-eval/eligibility-audit/segment-evidence.json
src-tauri/target/debug/footage-eval.exe --judge-eligibility .footage-eval/eligibility-audit/segment-evidence.json .footage-eval/eligibility-audit/frozen-judgments.json
python scripts/footage-eval/eligibility_report.py --output .footage-eval/genre-eligibility-live-2026-10-05 --baseline D:\自动剪辑系统\worktrees\footage-eval\.footage-eval\baseline-final-2026-10-01 --binary src-tauri/target/debug/footage-eval.exe --frozen-judgments .footage-eval/eligibility-audit/frozen-judgments.json
```

`--judge-eligibility` 是无模型、无应用数据访问的三体裁同片裁决回放，明确 `liveGeneration=false`；品牌身份和邻镜关系上下文为空。`eligibility-summary.json` / `eligibility-report.md` 汇总逐例基线与真实复跑的已命中硬风险入池数、无产物与淘汰/核验失败原因、最终机器风险预标变化和同片抽样。新入口的风险计数以入池前裁决快照为准，不能用之后追加的事实倒改当时的判断。旧池以冻结契约回放对照，未知不算阴性。

新入口的 `guardedHardRiskAdmissions` 与 `guardedUnqualifiedAdmissions` 非零会令报告命令失败；缺 Phase 2 的运行另报未测，零入池也不能称为出片通过。旧版局部重选不在本任务接线范围，`legacyPoolAdmissions` 单列，不能计为新入口验证。人工金标未纠正时硬风险识别正确率、误杀率及三体裁质量仍为 N/A；纯裁决抽样不能替代三套真实素材的用户看片验收。

## Agent 事实与体裁入口回归（任务 7）

```powershell
python scripts/footage-eval/agent_facts.py --snapshot D:\自动剪辑系统\worktrees\footage-eval\.footage-eval\frozen-2026-10-01 --output .footage-eval/task7-complete-live-2026-10-05 --workers 2
python scripts/footage-eval/audit_agent_facts.py --output .footage-eval/task7-complete-live-2026-10-05 --baseline D:\自动剪辑系统\worktrees\footage-eval\.footage-eval\baseline-final-2026-10-01
```

第一个命令在新输出目录构建真实二进制，沿用基线运行器与隔离规则，9 例×3 次，补发 genre 请求快照；workers 必须为 1–4，资源受限时建议 2。对照组为 history-1/2/3 与 local-replace 的原基线 12 次，另外测英文原稿冲突、切点微调、中英文普通问答和 HTTPS 网址回复。问答打开媒体选项仍不得生成；其 completed 无产物/生成目标指标为 N/A，不能套用生成请求分母。

第二个命令只读两组结果，把版本、镜头数、配音/字幕/BGM、当前版本预览/交付声明对落库行和真实文件检查，写当前输出的 agent-facts-audit.json；不写原基线。局部修改直接比较落库的素材身份/源区间与其余镜头和全部音文轨；另报体裁请求快照不一致和 search_assets 参数失败数。旧基线尚无体裁字段，该项为 N/A。无产物、进程中止、setup 失败的数量和测量分母保留。固定事实计数不涵盖画面语义、所有自由文本或最佳源窗，仍不冒充金标。

questionTurnPrelabel 是机器预标：讲解中作为示例的问题，以及「请求……未确认」也可能命中。无必要提问需逐轮审阅并记录理由，英文原稿与明确时长冲突的必要取舍单列；不把预标自动当真实反问数。任务 7 的入口传值不证明任务 6 的生成链已经使用体裁。验证记录见 [任务 7 变更](../changes/2026-10-05-genre-entry-agent-facts.md)。

仅补跑中英文问答与 HTTPS（3 例×3 次）：

```powershell
python scripts/footage-eval/agent_facts.py --answer-smoke --snapshot D:\自动剪辑系统\worktrees\footage-eval\.footage-eval\frozen-2026-10-01 --output .footage-eval/task7-answer-smoke-new --workers 2
```

交付验证分别绑定生成/局部整组与最终普通问答：生成/局部为 task7-complete-live-2026-10-05 的 18 轮；最终问答为 task7-user-answer-live-2026-10-05 的 9 轮。整组问答发现内部推理标签后，仅改 answer 分支的公开事实投影/文案请求与安全出口，再用 --answer-smoke 重建补测；两组不可冒充同一二进制。具体哈希、失败与分母见变更记录。

## 策划校准（任务 4b）

```powershell
python scripts/footage-eval/run.py --planning-only --snapshot D:\自动剪辑系统\worktrees\footage-eval\.footage-eval\frozen-2026-10-01 --eligible-evidence .footage-eval/planning-input-2026-10-05/candidates.json --output .footage-eval/planning-calibration-2026-10-05 --workers 2
```

`--eligible-evidence` 接受片段数组，或 `{evidence: SegmentEvidence[], windows: {"assetId:segmentId": EvidenceRange}, source: ...}` 包装。包装窗必须来自上游真实底线结果，位于同一冻结原窗内；Rust 收窄后重新封印，仍按当前体裁调用任务 3 `evaluate`，不会放行未知风险。每例报告合格片段数。任务 4b 使用任务 3 一次 `zh-15-silent/1` 的真实核验快照及 25 个合格窗，不合并跨运行的风险事实；来源与哈希保存在输入包装。

先运行 `prepareOnly` 得到需求定义和合格集合；三次正式运行分别重做逐片盘点、需求支持查找、体裁判定和策划/审核。只冻结素材与需求定义，不复用支持结论。每个需求覆盖全部片段，六个固定片段键组成一个并发请求，严格 Schema 强制完整真假判定；不再只检索少数支持片段。`preparation-summary.json` 独立记准备失败，未执行的三次不算模型运行失败。`summary.json` 分开记录 `gap_only`、`rejected`、外部服务错误和契约/运行器错误，增加 `eligibleSegments/genreStable/footageGapStable`。无产物引用率保持 N/A。这一模式仍不证明选镜/源窗最佳/配音/编辑器/三体裁桌面通过。

中断后可在上述命令中加 `--planning-preparation <旧输出目录>` 并使用新输出目录，复用已成功的 prepare。代码逐例检查请求、体裁选择、目标时长、完整原证据、上游候选/窗及轨道完全一致，输出保存准备来源与文件哈希；三次正式运行仍重新 `evaluate`、盘点、查找支持、判体裁和生成/审核。旧盘点仅供冻结需求定义，不把旧支持结论当三次独立结果。

2026-10-05 正式交付采用全新准备、12 例 × 3 次完整运行：24 份策划、12 次真实缺口、无最终审核拒绝或运行失败；8 个素材足够的宣传例全部 3/3，引用 100%、非法 ID/越窗 0。逐例任务 4 对比、18 次体裁补测、哈希与未验证边界见 [策划校准变更记录](../changes/2026-10-05-planning-calibration.md)。

## 策划 → 全片组合 → 证据源窗精修（任务 5）

```powershell
python scripts/footage-eval/run.py --planning-shots --snapshot D:\自动剪辑系统\worktrees\footage-eval\.footage-eval\frozen-2026-10-01 --eligible-evidence .footage-eval/shot-verification-resumed-2026-10-05/candidates.json --output .footage-eval/shot-relations-rerun-new --workers 4
```

12 例 × 3 次，真实 Provider、冻结风险快照与完整盘点，每次独立生成/审核策划、逐槽位看图评分、邻镜关系观察、代码组合及精修。此模式隔离任务 5 的效果，固定盘点及需求支持结论；任务 4b 的 `--planning-only` 仍三次重算盘点。没有声音、时间线、预览、编辑器或 GUI。`--planning-preparation` 只允许复用完整相同输入的成功准备阶段；`--skip-build --eval-binary <路径>` 可绑定已有评测二进制的 SHA256。运行中源文件冻结，不把旧二进制的结果绑定到新源码。

可选 `--verify-candidates` 在每次正式运行独立核验本体裁关键未知，花絮同时核验空镜比例所需的 EmptyShot 未知；因候选池改变而重新盘点，成功候选不重发，有限补发由代码参数控制。固定风险快照时，`verification-audit.json` 只统计快照来源那一次真实核验，不将其乘成 36 次。重试前合格数从当前最终合格集合中扣除依靠补发恢复的片段，后者仍须通过原底线；首轮/最终请求失败与重试成功分别计数，HTTP 429 耗尽原 Provider 退避不再套预算。与任务 4b 的受限比例比较不能单独归因为重试，因为新一轮重新策划、输入核验也可能变化。

`shot-result.json` 记录组合/精修/最终关系状态，`combination.json` 保存最少复用结果及同约束穷举不足证明；搜索预算耗尽明确失败，不当无解。`relations-input.json`、`refined-shots.json`、`final-relations*.json`、逐请求轨迹保存机器事实与实际源窗。精修后关系重新看实际端点，未改范围的成功关系缓存；关系错误只修后镜，其他镜头冻结。

`shot-metrics.json` 由 Python 独立对账实际源窗、风险证据、邻镜事实和隔离 SQLite，不采信 Rust 自报的零值。`shot-summary.json` / `shot-report.md` 汇总重复 assetId、相似复用与未知对数、相邻同场景次数、在窗高光率、动作截断、裁切出画/未知、越窗、跨硬切、合格池和受限比例，包含均值、最差及实测分母；完整精修与零输出分开报告。无镜头/未精修约束保持 N/A；动作、高光和裁切的机器样本不是人工真值，最佳窗、关系、裁切金标均 N/A。旧基线只比较已有的重复/越窗/硬切指标，旧 sameSegmentReuseProxy 不冒充视觉相似。

最小新增契约检查：`python scripts/footage-eval/test_shot_metrics.py`（空结果及旧端点关系不可借用）、`cargo test --manifest-path src-tauri/Cargo.toml --lib relations_contract`（组合/关系与源窗边界）。三体裁真实成片和桌面验收仍属任务 6/8。
