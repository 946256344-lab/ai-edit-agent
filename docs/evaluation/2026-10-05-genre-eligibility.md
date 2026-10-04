# 体裁底线评测

真实生成使用冻结分析；本入口暂按宣传。已命中硬风险是结构化事实口径，机器预标与人工金标分开。
空产物的风险不能记作最终选镜满分；没有 Phase 2 的运行不算入池门已测。三体裁抽样是纯裁决回放，不是三体裁生成。

代码绑定：HEAD `a86a20d06274deb166280c22a0308eba007fb5fc` + 本任务未提交源码，源码树 SHA256 `65facbc7bf365fa7e11d00fdc2b9b83791eac6b9fd309e2bcd8cac7ffd03caae`；实际二进制 SHA256 `b4230c17e3888f7e8153f2904a0518275a6886c56de0b4dbb28b3e335a91520f`。最终审计逐文件核对运行源码与交付源码相同，不把仅 HEAD 的旧状态作为本次行为依据。Provider/model 与基线一致（agnes-3.0-flash），运行进程并发 4；每次候选补核验最多四图。原始结果在当前 worktree `.footage-eval/genre-eligibility-live-2026-10-05/`；隔离审计在 `.footage-eval/eligibility-audit/delivery-audit.json`。

以下入池计数为三次运行的累计候选入池次数，同一片段出现在不同拍或不同运行时重复计数；每次运行的分项保存在 `eligibility-summary.json.cases`。最终预标均值只对有产物的运行计算，`None` 表示 N/A。

| 用例 | 基线产出/3 → 本次 | 基线入池硬风险 → 本次 | 本次入池已测/3 | 最终机器预标均值：基线 → 本次 |
|---|---|---|---|---|
| history-1 | 2 → 2 | 59 → 0 | 2 | 4.5 → 0.5 |
| history-2 | 0 → 0 | 0 → 0 | 0 | None → None |
| history-3 | 3 → 3 | 78 → 0 | 3 | 4.333 → 0.0 |
| zh-15-silent | 3 → 3 | 39 → 0 | 3 | 2.0 → 0.0 |
| en-30-silent | 3 → 3 | 60 → 0 | 3 | 4.0 → 0.0 |
| en-15-voice | 0 → 0 | 0 → 0 | 0 | None → None |
| missing-visual | 0 → 0 | 0 → 0 | 0 | None → None |
| local-replace | 3 → 2 | 42 → 0 | 2 | 2.0 → 1.0 |
| narrative-gap | 0 → 0 | 0 → 0 | 0 | None → None |
| bts-industrial | 0 → 2 | 0 → 0 | 2 | None → 0.5 |

## 无产物与淘汰原因

本次 15/30 有产物（基线 14/30），最终镜头 115（基线 132）。15 次进入新门，共 1,275 次候选入池，`guardedHardRiskAdmissions=0`、`guardedUnqualifiedAdmissions=0`；其余 15 次未进入该阶段，不能算门已测。旧版局部重选有 9 次候选入池，单列 `legacyPoolAdmissions`，不属于新门通过证据。基线结构化硬风险累计入池 278，本次所有已观察池为 0，但旧局部路径仍未改造，不能保证后续不放回。

最终机器风险预标总数 46 → 4；带预标的有产物运行 14 → 4，四条预标来自 history-1、local-replace 和 bts-industrial，仍需用户看片纠正。新门中出现的硬命中理由累计为失焦 306、品牌 333、展会 120、杂乱 302、抖动 20；另有 105 个无单一片段证据的跨段候选排除理由。理由可重叠，不能相加当作独立片段数或误杀率。

没有观察到「进入底线后因零合格素材无产物」的真实运行：15 次有底线轨迹的运行均产生了产物；其余无产物运行均缺底线轨迹。不能据此断言过滤没有造成内容覆盖损失，P1 和最终覆盖仍属后续集成验收。关键未知补核验的失败共 126 次：响应为空 24、响应无效 9、响应越窗 30、超时 52、连接中断 10、HTTP 500 一次。失败候选保留未知/淘汰原因，未重试为假成功。

最终隔离审计通过：30 次均无窗口，数据目录在各自输出内；冻结数据库、分析、向量、金标等哈希、1,353 张帧和 94 条原片内容哈希不变；原资产 metadata_json 无改动。运行副本累计追加 7,564 行风险核验事实（同一片段跨运行独立计数），不代表 7,564 个独立片段。

- history-1 #3：status=completed；淘汰={}；核验失败={}。没有底线轨迹的失败不能归因于底线。
- history-2 #1：status=partially_completed；淘汰={}；核验失败={}。没有底线轨迹的失败不能归因于底线。
- history-2 #2：status=completed；淘汰={}；核验失败={}。没有底线轨迹的失败不能归因于底线。
- history-2 #3：status=completed；淘汰={}；核验失败={}。没有底线轨迹的失败不能归因于底线。
- en-15-voice #1：status=needs_clarification；淘汰={}；核验失败={}。没有底线轨迹的失败不能归因于底线。
- en-15-voice #2：status=needs_clarification；淘汰={}；核验失败={}。没有底线轨迹的失败不能归因于底线。
- en-15-voice #3：status=needs_clarification；淘汰={}；核验失败={}。没有底线轨迹的失败不能归因于底线。
- missing-visual #1：status=completed；淘汰={}；核验失败={}。没有底线轨迹的失败不能归因于底线。
- missing-visual #2：status=completed；淘汰={}；核验失败={}。没有底线轨迹的失败不能归因于底线。
- missing-visual #3：status=completed；淘汰={}；核验失败={}。没有底线轨迹的失败不能归因于底线。
- local-replace #1：status=completed；淘汰={}；核验失败={}。没有底线轨迹的失败不能归因于底线。
- narrative-gap #1：status=completed；淘汰={}；核验失败={}。没有底线轨迹的失败不能归因于底线。
- narrative-gap #2：status=completed；淘汰={}；核验失败={}。没有底线轨迹的失败不能归因于底线。
- narrative-gap #3：status=completed；淘汰={}；核验失败={}。没有底线轨迹的失败不能归因于底线。
- bts-industrial #3：status=completed；淘汰={}；核验失败={}。没有底线轨迹的失败不能归因于底线。

## 三体裁同片裁决抽样（冻结旧分析，无关系上下文）

同一批 141 个冻结片段：宣传全部 rejected（旧关键缺项未知且 finalCheck=true）；叙事全部 pending_verification（矛盾/跳轴/无关插入缺邻镜关系），花絮全部 pending_verification（摆拍/广告感缺核验）。花絮没有因宣传的失焦/抖动/品牌等理由硬拒；pending 不算合格。叙事关系核验不在任务 2 的 RiskKind 枚举内，本模块只消费 `RelationEvidence`，由任务 5 提供真实关系事实。下面展示真实旧分析，三项契约测试另覆盖花絮接受失焦、广告感命中拒绝以及叙事已证实跳轴拒绝；测试不替代真实三体裁素材。

| 资产 / 片段 | 宣传 | 叙事 | 花絮 | 宣传原因 |
|---|---|---|---|---|
| 21b170f6-fb70-489d-8434-eb4ba7a71098 / s001 | rejected | pending_verification | pending_verification | out_of_focus, unknown_shake, unknown_brand_logo, unknown_exhibition, unknown_clutter |
| 4a7ea239-5933-4f2f-bdd5-d6c20b0818c6 / s001 | rejected | pending_verification | pending_verification | unknown_out_of_focus, unknown_shake, brand_logo, exhibition, unknown_clutter |
| 4a7ea239-5933-4f2f-bdd5-d6c20b0818c6 / s002 | rejected | pending_verification | pending_verification | unknown_out_of_focus, unknown_shake, brand_logo, exhibition, unknown_clutter |
| 4a7ea239-5933-4f2f-bdd5-d6c20b0818c6 / s003 | rejected | pending_verification | pending_verification | unknown_out_of_focus, unknown_shake, unknown_brand_logo, exhibition, unknown_clutter |
| 7c7038aa-5eb3-44ac-9e35-21c0871ae7e9 / s001 | rejected | pending_verification | pending_verification | unknown_out_of_focus, unknown_shake, brand_logo, unknown_exhibition, unknown_clutter |
| 7c7038aa-5eb3-44ac-9e35-21c0871ae7e9 / s002 | rejected | pending_verification | pending_verification | unknown_out_of_focus, unknown_shake, brand_logo, unknown_exhibition, unknown_clutter |
| 7c7038aa-5eb3-44ac-9e35-21c0871ae7e9 / s003 | rejected | pending_verification | pending_verification | unknown_out_of_focus, unknown_shake, brand_logo, unknown_exhibition, unknown_clutter |
| 94837af4-9fb0-46d4-a2ca-c9972959c017 / s001 | rejected | pending_verification | pending_verification | unknown_out_of_focus, unknown_shake, brand_logo, unknown_exhibition, unknown_clutter |
| 00f9fc62-97ed-40f7-8d37-6a76cc434463 / s001 | rejected | pending_verification | pending_verification | unknown_out_of_focus, unknown_shake, unknown_brand_logo, unknown_exhibition, unknown_clutter |
| 0201d06b-ad2b-484a-b402-e2c7409600ac / s001 | rejected | pending_verification | pending_verification | unknown_out_of_focus, unknown_shake, unknown_brand_logo, unknown_exhibition, unknown_clutter |
| 057ea0b4-6c04-4d34-a1d0-c6da92593957 / s001 | rejected | pending_verification | pending_verification | unknown_out_of_focus, unknown_shake, unknown_brand_logo, unknown_exhibition, unknown_clutter |
| 06becaa9-6ed1-44a6-ab23-3a302bdfe434 / s001 | rejected | pending_verification | pending_verification | unknown_out_of_focus, unknown_shake, unknown_brand_logo, unknown_exhibition, unknown_clutter |
