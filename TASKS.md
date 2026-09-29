# 任务清单

## 当前任务窗口

### 进行中

- [ ] P0 待定方案（2026-09-27）：产品核心收敛为「策划 + 选镜」，见 `docs/decisions.md` 首条。09-27 三次宣传片实测（94 条工业素材）暴露的问题——先编故事再硬凑画面、Agent 反问与说错结果、文字与画面对不上、别家品牌与展会画面入选——都按新方向重做，不再在精修层打补丁。方案思路已在会话「宣传片剪辑模式测试」里与用户讨论（09-29 起以它为准，不再派只读梳理会话）：先分体裁（叙事 / 宣传 / 花絮），体裁决定策划结构与「什么是好镜头」；素材不做人为分类。建议顺序：① 评测集与基线分数；② 底线过滤（失焦、抖动、别家品牌、展会等命中即淘汰）与可用性评分；③ 素材先行的策划（策划每段必须引用库里真实存在的镜头）；④ 镜头关系规则与体裁配方；⑤ 备选镜头随草稿交付。体裁确定方式已定（输入框默认「自动」由 AI 判断，可手选）；三种体裁的标准表用户认为不准确，待用户给出。确认后写进 `docs/decisions.md` 并出任务书。
- [ ] P0 待修（2026-09-27）：删除会话等确认框失效，删除不经确认直接执行。开发版 webview 中 `window.confirm` 被 Tauri 替换为返回 Promise 的函数（恒为真），`src/App.tsx` 的 `deleteEditingSessionWorkspace`、`ProjectSettingsModal` 清缓存、`useNavigationEditController` 的确认判断全部失效，还报「dialog.confirm not allowed」。09-27 录屏期间「Weekend Road Trip」两个会话因此被永久删除。09-29 用户暂不安排，未派发。修复时的验收：三处点取消后数据仍在、点确认后才执行；不再报 dialog 权限错误；其余 `window.confirm` 一并排查；未桌面实测的标待桌面确认。
- [ ] 首发准备（2026-09-26，目标 10-01，海外优先）：09-28 已定 10-01 小范围免费内测，不设邀请名单、资格整个内测期有效，配音经网关代理开发者 key（见 `docs/decisions.md`「首发」）。仍待定：上游模型单次成本（D4）、交付物口径（D6）。09-28 已派执行会话「网关与配音」（网站 + 桌面两个仓库，已合并：网站 e6d35b1、桌面 6bdd34e；上线前用户须按顺序：先部署 Firestore 规则，再设 Vercel 环境变量并部署，见 `docs/changes/2026-09-28-gateway-voice.md`）与「官网页面」（已合并到网站 main e6d35b1；Terms / Privacy 与下载信息的占位待用户填写）。09-29 用户定：不设每人额度、配音改用 ElevenLabs，已派执行会话「关闭每人额度、网关配音改 ElevenLabs」（网站 + 桌面，已验收、合并中）；桌面仓库目前公开，上线后再改私有。P0 剩余：正式域名构建、网站生产部署、经网关的 413 实测、CapCut 与 Resolve 实测交付、英文主路径验收、代码签名、英文 Terms/Privacy 与删除渠道、第三方许可。逐项见 `docs/release-checklist.md`。
- [ ] 提速（2026-09-26）：用户要求模型请求按需全并发、不考虑成本。基线 `generate_storyboard` 90 秒（Phase 3 串行 35 秒、Phase 4 24 秒）。已改、待实测：Phase 3 各拍并发；画面分析队列 16 并发；本地模型走显卡（94 条技术分析 14.3→6.9 分钟）；技术分析并行降为核数 1/4 并限线程（94 条 5.5 分钟、无失败）；样本帧并入运动分析同一次解码；导入时按硬切段整段画面识别，新字段已接入召回与 Phase 3。下一步：用「测试1」同稿重新识别，实测耗时与质量；Phase 4 用最佳区间与主体位置定区间和裁切。见 `docs/changes/2026-09-26-technical-analysis-throughput.md`、`2026-09-26-local-models-on-gpu.md`、`2026-09-26-whole-shot-visual-analysis.md`。
- [ ] 文档整理（2026-09-28）：第一步已完成——`TASKS.md` 只留未完成的事，待桌面确认的改动并入 `docs/release-checklist.md` 附录 A；`docs/decisions.md` 按主题重排、删去被取代的内容；删除 `docs/roadmap.md` 等过时文件。第二步待派执行会话（等选镜链路现状报告回来）：`docs/architecture.md`、`docs/api.md` 改为只写现状，日期段落归入 `docs/changes/`；README「当前实现」按现状重写；更新 `docs/codebase/ARCHITECTURE.md` 与 `CONCERNS.md`；删除过时的 `docs/changes/TASK_HISTORY.md`（`scripts/check-doc-sync.mjs` 遇到 `docs/changes/` 下被删除的文件会读文件报错，需先修）。

### 待办与待查

- [ ] 待办（2026-09-27）：原生文字剪映「已验证」判定两处不一致——`storyboard_text_tracks` 把默认描边 + 阴影字幕直接标 `verified`，`validate_text_tracks` 对同样样式判 `local_preview_only`；需在真实剪映 / CapCut 草稿确认描边与阴影后统一。
- [ ] 待查（2026-09-27）：FCPXML 变速镜头的 `timeMap` 第一个 `timept time` 为 `0s`，而 `asset-clip start` 写的是源起点；按 FCPXML 本地时间语义可能取错画面（`src-tauri/src/handoff/fcpxml.rs` `asset_clip_xml`）。需用 Resolve / FCP 导入一个放慢镜头核对后再改。
- [ ] 待办（2026-09-27）：未移除时再次导入同一文件夹会为每个文件新建重复素材（`store_assets` 不按源路径去重），需决定跳过、提示还是重链。
- [ ] 待办（2026-09-27）：画幅已知限制——Phase 3 候选卡的竖屏可裁性标签不随画幅变，字幕每行字数仍按竖屏。
- [ ] 待办（2026-09-26）：Tesseract 只带英文数据，识别不到中文招牌。
- [ ] 待办（2026-09-28）：`confirmation.rs` 的配音失败提示写死中文，英文界面会漏出中文（网关配音会话发现）。
- [ ] 待办（2026-09-28）：开发构建内置网关时，设置页的自带配音 key 区块仍显示但不生效。
- [ ] 待办（2026-09-28）：`docs/codebase/STRUCTURE.md` 第 107 行附近两行表格挤在一行，并入文档整理第二步。
- [ ] 待查（2026-09-28）：浅色工作区替换面板可能尚未适配片段候选卡（`shot_replacement.rs`），原记在旧 decisions「未完成事项」，未核实是否仍成立。

### 待桌面确认

已写完代码、只差真实桌面确认的改动统一列在 `docs/release-checklist.md` 附录 A，跑首发主路径时逐条勾掉；不通过的转成本页待修项。

## 说明

- 本页只放未完成的事：进行中、待办与待查。每条写清现状、下一步和验收标准，细节用链接指向 `docs/changes/` 或其他文档。
- 代码写完但未桌面确认的，移到 `docs/release-checklist.md` 附录 A；确认完成的直接删除，历史查 git 与 `docs/changes/`。
- 遇到问题先记录复现方式、实际错误和影响，再决定修代码、补测试还是暂时接受限制。
- 派发给执行会话的任务，在条目里注明分支和验收标准，协作方式见 `CONTRIBUTING.md`「主会话与执行会话」。
