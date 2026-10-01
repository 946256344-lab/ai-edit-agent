# 长期文档只描述现状，历史归档

## 触发范围

- `README.md`、`docs/architecture.md`、`docs/api.md`、`docs/codebase/{ARCHITECTURE,CONCERNS,STRUCTURE}.md`：长期文档整理，无业务源码或公开契约修改。
- `scripts/check-doc-sync.mjs`：修复变更集删除 changes 文件时读不存在文件的真实 bug。
- `.harness/doc-sync-policy.json` 的 desktop-contract 规则本次未触发；本记录按 docs/changes/README.md 与任务书要求新增。
- 删除 `docs/changes/TASK_HISTORY.md`；不修改 TASKS.md、docs/decisions.md、docs/release-checklist.md。

## 改动

- architecture 按导入分析、Agent、策划/选镜 Phase、时间线/preview/交付、数据作用域与系统边界组织，标明当前仍是 brief 先行与尚无真实 ASR，不把产品方向当实现。
- api 按模块列出当前全部 101 个注册命令，按查询/创作列出全部 33 个工具；参数与返回重新从实际 Rust 声明读取，保留 strict Schema 的字段、nullable、数组/长度/数值约束与嵌套结构。兼容命令只要仍注册就保留，已移除工具不进入当前清单。
- README 当前实现缩短并链接 architecture/api，修正画幅、多帧分析、多轨音文与四输出端口事实；运行、桌面环境依赖等章节保留，仅补改过时事实。
- 更新源码导览/风险，删掉旧行数与已完成拆分路线，修 STRUCTURE 挤在同一行的 music_plan / voice_provider 表格及过时动态加载描述。
- 文档同步脚本只对 ENOENT 跳过工作区删除，不吞权限或其他读取错误；暂存区不存在的记录不参与；recordContents 不存在的文件不能计为有效变更记录，避免仅删除记录就满足记录要求。

## 历史保全与去重

原 architecture 的 21 个日期标题、api 的 46 个日期标题，以及未独立设标题的维护记录均检查归档。既有 changes 已涵盖的摘要直接从长期文档移除，不重复追加；不能确认同义覆盖的旧参数、阶段窗口、状态/恢复细节，以“原长期文档补充”保留，并标明不代表现状。

- 对应主题已存在、直接去重：CapCut/输出端口/链接器、源窗放慢、grid-over-caption、首次分析不等待、配音稿确认、段候选池、软拆拍、自动 preview、GPU、appliedMedia、品牌卡/转场、音乐先行、网关配音等既有同日期记录。
- 对应主题有记录但保留额外历史细节：`2026-09-07-phase4-refine-batch.md`（记录初版 10 镜与后续 4 镜）、`2026-09-07-smooth-editing-pipeline.md`（后续 audio-first/crop/缓存契约）、`2026-09-08-phase3-consecutive-asset-ban.md`（被片段规则取代的说明）、`2026-09-15-composer-media-options.md`（后来画幅/BGM 契约）、`2026-09-17-storyboard-versions.md`（后来派生版本加性字段）、`2026-09-25-fellowcut-desktop-account.md`（账号契约）、`2026-09-26-shot-detail-in-asset-panel.md`（完整详情与维护记录）。
- 没有可确认完整覆盖的日期段落/独立维护记录归入对应日期 `*-long-doc-history.md`，保留原述与来源；包括早期本地音乐、视觉/OCR、Provider/配音、Phase4 局部恢复、分析门、详情预览、运行时模型下载等。历史中的废止契约不重新算成当前 API。
- 没有重写既有验证结果，不把旧记录的实测结果作为本次验证。

## 同步文档

- README.md
- docs/architecture.md
- docs/api.md
- docs/codebase/ARCHITECTURE.md
- docs/codebase/CONCERNS.md
- docs/codebase/STRUCTURE.md
- docs/changes/ 对应日期归档与本记录

## 验证

- `npm run harness:check`：架构预算 6 个文件、Agent 上下文/IPC/边界/目录、文档同步、i18n 全部通过。
- `git diff --check`：通过；仅 Git 本机 LF/CRLF 提示，无空白错误。
- 契约逐项对照：解析 lib.rs 的 generate_handler 得 101 个命令，逐个定位注册模块函数声明并核对 camelCase 输入/实际 Rust 返回；local-store.ts 的 95 个静态 invoke 名称均在注册清单。33 项 tools.rs Schema 与 policy.rs、native.rs 白名单、TS 镜像集合完全一致；工具参数/required/嵌套 Schema 逐项核对。本次不是只搜索文档是否提到名称。
- 已删除记录回归：工作区、`--files`、`--staged` 三路径均不读已删文件；删除记录本身不能充当有效记录，已有有效记录仍可满足同步要求。工作区/显式文件/暂存区断言回归与 scripts/test-doc-sync.mjs 均通过；暂存区无 fatal/ENOENT 输出，删除不能满足记录门，有有效记录时通过。
- 无应用/开发服务器启动，无业务源码修改，无 npm install；依赖仅由当前 worktree 的 node_modules junction 引用主仓库。

## 未验证

未实测真实媒体分析质量/耗时、用户确认 UI、网关部署/资格/额度、配音/BGM、编辑器打开/变速/文本/音轨/转场、干净机安装、签名或官方 OAuth 支持范围。长期文档明确标注未核实，桌面项仍由 release-checklist.md 所有。

## 新发现

- decisions 的「没有真实素材就不写策划」与体裁方向仍未落为强制镜头引用/体裁分流，当前只有库存提示约束；不修改决策。
- decisions 写粗拍软反馈再拆一次，当前 MAX_PHASE1_REVISIONS=3，粗拍反馈可用尽剩余尝试（不只一次）。
- decisions 的「Provider 失败不静默切换」与无网关配音仍保留 Fish→ElevenLabs 传输类回退，范围需要主会话确认；网关模式无回退。
- read_logs 的 Schema 描述仍称需用户明确要求，实际完整目录/skills 执行没有该明确要求门；api 按代码行为写，不宣称存在此守卫。
- transcribe_asset 有真实识别描述但执行只返回占位，且没有自动加载 whisper 的路径。
- 旧长期文档声称旁白不交付剪映/CapCut，当前能力表、Rust payload 和 Python add_voiceover_tracks 已支持写入；仅源码核实，编辑器效果未核实。

## 决策

无。产品方向/现行决策仍由主会话维护，本次只回报偏差。
