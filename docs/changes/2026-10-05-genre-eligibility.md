# 体裁底线与可用性分（2026-10-05）

任务 3 在 `feature/genre-eligibility` 实现新生成入池前底线。代码参数版本 `genre-eligibility-v1`；不修改原分析，不重建素材分类，不把模型文字当产物事实。

## 接口与行为

- `storyboard/eligibility.rs::evaluate` 纯裁决片段证据、体裁、画幅、项目品牌身份、显式源窗和关系事实；返回 `EligibilityDecision` 的状态、原因、引用证据 ID、待核验项与合格可用窗。宣传按主体失焦、不可接受抖动、非本项目标识、展会/展台/展厅、杂乱背景执行硬门。旧整段正向标记在任何子窗仍命中；不自动裁掉风险。
- 抖动的明确带置信度 `severity=mild` 可接受，其余命中均保守淘汰；帧差/handheld 本身不是命中。品牌名只从品牌套件名称或调用方给出的明确身份取，精确匹配所有可见名称；混合标识、未知标识、没有可匹配名称的 logo 文件都不能放行。
- 宣传关键未知先调用任务 2 的 `verify_candidates`；最终仍未知或核验失败则不合格。花絮摆拍/广告感命中硬拒，未知保持待核验；手持与轻微失焦不作为花絮硬门。空镜 ≤20% 交任务 5 的组合门。叙事的矛盾/跳轴/无关插入通过本模块 `RelationEvidence` 消费邻镜关系；已证实命中硬拒，缺上下文待核验，不从方向反转推断跳轴。任务 2 尚无关系风险请求枚举，本模块未改 `models.rs`。
- `prepare_candidates` 只补本拍召回需要的候选（每拍最大池 12，同素材最多 2），按候选实际源窗核验，淘汰或已核验仍待定后向下补，直到需求覆盖或候选耗尽；独立请求由既有 Provider 并发与仅 429 退避，每请求最多四图。包括 Provider 解析/核验入口失败在内，逐候选留因，不写假阴性。`EligibleInventory` 提供 `sources/evidence_snapshot/decisions/usability` 给后续任务。无单一片段契约的旧双段组合不入池；零合格片段明确失败并报原因统计。
- `scoring.rs::usability_score` 保留技术可读、主体可见、按所选画幅的水平裁切保留、动作完整、高光覆盖与源窗容量。未知分项为 null，分项已知数量独立保留。总分只聚合已有可用性证据；它是排序代理，不证明风险阴性或最佳区间。源宽高和主体水平跨度控制画幅分，不使用写死的竖屏标签；需上下裁切却没有纵向边界时保持未知，原整段的干净首尾也不借给任意子窗。
- 合格集合以可用性优先排序，内容召回分独立保存；新池 `CandidateScore.usability` 为加性字段，旧池默认 null。新池 `total` 不含既有质量/时长分，质量/时长分仍留作兼容诊断。匹配/多样性名额只能在合格集合填充，候选不足绝不撤销底线。

## 最小接线与范围

1. `storyboard.rs`：注册 eligibility、开放 scoring 给任务 4，现有新生成 Phase 2 前调用 `prepare_candidates`；明确暂按宣传，画幅取已有媒体快照，品牌身份取现有品牌套件。
2. `storyboard/phases.rs`：加 `phase2_eligible_shot_selection` 与内部可选合格清单参数；底线再次约束排序集合，池轨迹增加可用性分项，旧测试夹具只补加性缺省。旧召回函数保留给历史路径。
3. `footage_eval.rs`：仅增加无模型的 `--judge-eligibility` 评测分支；读取导出的片段契约，输出三体裁同片裁决，品牌/关系上下文为空。

没有改 `models.rs`、前端、agentloop、media_options、phase4、时间线、交付及其他 worktree。没有改 decisions、TASKS 或已审定方案。旧故事版读取保持原样。旧版局部重选、已有备选池、最终源窗重验、真实体裁和故事版元数据落库仍由任务 5/6/7 接入；不能把本模块入池门当成全管线或三体裁上线通过。

## 评测与检查

冻结输入：只读 `D:\自动剪辑系统\worktrees\footage-eval\.footage-eval\frozen-2026-10-01`；基线：同目录 `baseline-final-2026-10-01`。新输出全部在本 worktree `.footage-eval/`，运行器 `--workers 4`，不启动 GUI/开发服务器、不写真实应用数据。三体裁同片抽样只证明代码裁决，不冒充真实三体裁生成或人工金标。

真实生成 10 例 × 3 次已完成，逐例对比与三体裁同片抽样见 `docs/evaluation/2026-10-05-genre-eligibility.md`。15 次进入底线，共 1,275 次合格候选入池，已命中硬风险入池和未合格入池均为 0；基线硬风险入池累计 278。另有旧版局部重选 9 次入池，不作为新门已验证。15 次无产物均未进入底线阶段，不能归因于淘汰；没有观察到进入底线后零合格素材导致无产物的真实用例。

总产出 14/30 → 15/30，最终机器风险预标总数 46 → 4；该预标仍需人工核对。当前真实入口默认宣传，工业花絮用例的名字不代表真实走花絮规则。冻结旧证据三体裁裁决为宣传 141 个拒绝、叙事/花絮各 141 个待核验，不给未知贴合格标签。叙事候选关系补核验尚缺任务 2 的请求枚举与邻镜上下文，已提供 `RelationEvidence` 裁决接口，须任务 5 接入后才能完成该体裁的候选核验闭环。

最终隔离审计通过：30 次运行均无窗口、数据路径在评测输出内；冻结数据库/分析/向量/金标、1,353 张帧与原片哈希不变，原资产 metadata_json 未变。运行源码逐文件与交付源码一致，二进制 SHA256 与运行记录一致；原始证据和代码绑定在 `.footage-eval/genre-eligibility-live-2026-10-05/` 与 `.footage-eval/eligibility-audit/delivery-audit.json`。

交付检查：`cargo check --manifest-path src-tauri/Cargo.toml`、带 `--features footage-eval --bin footage-eval` 的编译检查、三项 `cargo test --manifest-path src-tauri/Cargo.toml --lib genre_floor_contract` 契约测试通过；`python scripts/footage-eval/test_metrics.py` 八项通过；评测报告命令与隔离审计通过；`npm run harness:check`、`git diff --check` 通过。已有评测二进制构建通过且绑定当前源码。没有前端修改，不要求 lint。没有运行 GUI、开发服务器、真实三体裁/编辑器/Release 验收，人工风险识别漏检和误杀仍为 N/A。
