# 证据契约与候选按需补核验（2026-10-05）

本任务追加可被代码消费的事实，不实现体裁底线、评分、策划或选镜。现行 Phase 1–5、Agent 工具和前端入口没有行为改动；三个 storyboard 文件仅为旧测试夹具补 `provenance: None`。导入时的请求、提示词、队列与默认分析流程保持原样，新识别只多记录来源。

## 契约与所有者

- `models.rs` 定义 `SegmentEvidence/RiskEvidence/EvidenceSource/EvidenceRange/EvidenceState/RiskKind`。每条风险有内容 ID、三态、来源、源时间窗、可得置信度和可见值。无时段的风险按整段；旧否定值/缺字段/占位值/无置信度一律未知，旧正向标签保留命中。帧差不是抖动，方向不是轴线；不猜文件名品牌。
- `assets/evidence_contract.rs` 为纯适配、内容寻址、源窗三态查询和追加元数据持久化所有者。主体边界/位置、方向、干净开头结尾、高光/变化/最佳区间完整保留；未分段整片的多条旧证据不丢弃，有真实分段时缺卡段不借整片标签。`get_asset_evidence` 加性返回 `segmentEvidence[]`，旧字段仍可读。
- `assets/evidence_verification.rs::verify_candidates` 只接收当前候选、基础快照、所需项与可选候选源窗。每段最多四张时间网格，独立请求并发，经统一 Provider 仅在 429 退避。越窗、错项目、未请求风险、解析失败或分析已更新均如实失败。只追加事实，原分析不改；命中/阴性冲突保留，后续核验可补齐其明确覆盖的未知窗，部分阴性不替整段放行；所有历史仍留在核验表。已知候选不再请求模型。
- schema v21 只追加 `asset_evidence_verifications`、`storyboard_evidence_metadata` 及索引。`StoryboardEvidenceMetadata` 固定 `pipelineVersion/genre/recipeVersion/evidenceSnapshot/evidenceReferences`；引用含 `evidenceId/assetId/segmentId/range/supports`。版本/内容 ID/引用范围受写入校验，同一故事版附加事实写一次，不回填、不重建表、不重新编号。历史无行读取为 null/空数组。

任务 3 通过 `load_asset`、`risk_state_for_window` 与 `verify_candidates` 取事实，另行裁决底线和评分。任务 4/6 在创建新版本的事务中用 `write_storyboard_metadata`，任务 6 将 `read_storyboard_metadata` / `project_storyboard_version` 与已预留的 `StoryboardVersionWithEvidence` 平铺投影接入读取/派生，任务 7 消费冻结类型。当前生成尚不填写附加元数据，没有新管线合格标签。

## 冻结证据与规则回放

只读使用评测 worktree 的 `frozen-2026-10-01` 和 `baseline-final-2026-10-01`；全部新输出在当前 worktree `.footage-eval/`。

`--contract-replay` 对 141 个片段导出两次字节相同的契约，ID/内容稳定。片段×风险共 1,551 对：70 命中、1,481 未知、0 已核验阴性。非未知覆盖 4.51%，全部 11 项未知 95.49%；核心七项 987 对中 918 未知，未知 93.01%。这反映旧数据证据缺口，不是识别正确率。

旧机器正向预标共 34 对，结构化正向覆盖 25 对（73.53%）。未覆盖 9 对包括 7 个宽泛失焦关键词、1 个抖动词与 1 个品牌占位，不将它们猜成真实视觉事实。金标为空，漏检和误报（后续误杀风险代理）均为 N/A。新增独立原始风险事实金标 CSV 与计算，不用体裁风险 `none` 冒充全部事实阴性。

保存产物的 30/30 个运行核心指标与基线一致，15/15 个保存故事版通过现行 P5。冻结数据库、分析与金标哈希未变。该模式明确 `liveGeneration=false`，是规则/读取/计分回放，不冒充模型重新生成或逐镜质量验收。

## 真实补核验与真实生成复跑

第一轮真实候选核验使用两个曾入选片段的独立 SQLite 副本，模型 `agnes-3.0-flash`。请求品牌、抖动、杂乱背景；其中已知品牌项跳过，实际补五项，得到五条带模型/方法/源窗/置信度的记录。原 assets.metadata_json 全部逐字段未变，窗口数为 0。重复同一请求：契约/ID 相同、核验仍五行、Provider 未再次解析，没有重复模型请求。此观察不是人工确认识别正确，也不是最终源码的五项成功保证。

另外已用同一冻结快照、同一 Provider/模型完整复跑 10 例×3 次真实生成（6 个独立进程并行）。结果不与旧基线逐镜一致，不将差异归因为本次契约改动，也不能声称本任务改善了策划/选镜：

| 用例 | 旧基线产出 /3 | 本次真实复跑产出 /3 |
|---|---:|---:|
| history-1 | 2 | 2 |
| history-2 | 0 | 1 |
| history-3 | 3 | 3 |
| zh-15-silent | 3 | 3 |
| en-30-silent | 3 | 3 |
| en-15-voice | 0 | 0 |
| missing-visual | 0 | 0 |
| local-replace | 3 | 2 |
| narrative-gap | 0 | 0 |
| bts-industrial | 0 | 1 |

总计从 14/30 产出变为 15/30，最终镜头从 132 变为 146。局部换镜实际成功从 1/3 变为 2/3（一次 setup 无产物）。有 Phase 1 的运行策划引用仍为 0%。history-2 新产出的目标时长偏差 6,654 ms；history-3、en-30-silent 与 bts-industrial 的最终拍覆盖均值分别 97.44%、97.22%、87.50%，不能把“有产物”当完整通过。实时差异的原因未经对照实验裁决，严格“生成结果与基线一致”这一项不能声明满足；交主会话决定是否接受规则回放作为本次无生成改动的回归证据。

实际运行的二进制/源码树绑定保存在 `live-regression-2026-10-05/runtime.json` 与 `experiment.json`，与随后补齐“未知后续核验可恢复”、故事版投影契约和部分阴性源窗的最终源码区分记录。当前生成未调用这些新增接口；最终源码另跑契约测试、冻结规则回放和候选实测，不把旧二进制自称为最终提交。

隔离审计：30 个运行均无窗口，实际数据根全部在输出内；冻结数据库哈希不变，1,353 个冻结帧逐个内容哈希未变。详细真实报告、对比与审计分别为 `.footage-eval/live-regression-2026-10-05/{report.md,baseline-comparison.json,isolation-contract-audit.json}`。保存产物回放的 30/30 一致不能挪用于上述真实生成复跑。

## 检查与未验证

四项 Rust 契约测试覆盖未知兼容、内容 ID、完整关系字段、追加迁移/新故事版往返与不可覆盖、冲突保留、按需项、缺置信度、越窗、作用域、过期分析、未知被后续核验补齐、幂等回写和四图上限。Python 计分契约测试八项（原七项加原始风险三态/独立金标）。

未启动 GUI 或开发服务器，未改用户真实应用数据，未改主仓库或其他 worktree。无前端修改，不要求 lint。没有人工金标、真实桌面/三体裁/编辑器验收；没有模拟 429（沿用统一 Provider 既有实现）。补核验和新故事版附加字段的生产接入仍归后续任务，不因本任务完成而自动启用。


实际检查：`cargo check --manifest-path src-tauri/Cargo.toml` 通过（保留现有与预留接口的 dead_code 警告）；`cargo test --manifest-path src-tauri/Cargo.toml --lib evidence_contract` 四项通过；`python scripts/footage-eval/test_metrics.py` 八项通过；`cargo build --manifest-path src-tauri/Cargo.toml --features footage-eval --bin footage-eval` 通过；`npm run harness:check`、`git diff --check` 通过。没有前端改动，未运行 lint。


时段边界补充：模型返回段内部分阴性时保留该时段的 `not_hit` 与来源/置信度，其余时段继续未知。只在查询窗被一条明确阴性完整覆盖且没有重叠命中时返回未命中；不拼接零散阴性、不从未覆盖区推断安全。候选补核验可限定 `range`，响应不得越出请求窗；回写与返回仍是基础片段的完整契约，分析快照 ID 不改。部分阴性边界有契约测试，限定源窗有真实候选回读，生产生成仍未调用新接口。

最终源码交付复核的冻结输出位于 `.footage-eval/contract-replay-delivery-2026-10-05/contract-replay-report.json`：两次字节相同，冻结文件未改，覆盖率和未知比例同上，30/30 核心指标相同、15/15 P5 通过。最终源码对应二进制的候选输出位于 `.footage-eval/live-verification-delivery-2026-10-05/`，`code-version.json` 绑定源码与二进制 SHA256。两个候选中一个返回 `verification_response_empty`，该候选没有写入假阴性；另一个实际写入抖动、杂乱背景两条未命中证据（置信度 0.85/0.8）。用这两条已核验窗的严格子窗再次调用，均跳过 Provider、核验行数仍为 2、返回完整契约与 ID 不变，原资产分析逐字段未变，窗口数为 0。`verification-audit.json` 保存这些事实与候选失败。未对空结果自动重试，失败仍作为交付证据保留。

交付状态为部分完成：证据契约、追加持久化、按需核验接口与评测接入已实现并完成上述检查；验收 6 的真实生成结果严格一致未满足，原因尚未裁决，不将保存产物回放的一致性替代这一结论。
