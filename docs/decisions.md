# 当前技术决策

本文只记录今天仍然影响开发的决定；历史 ADR 仍可在仓库历史中查阅，不再作为必读规则。

## 选镜局部编辑走工具，不重跑整条流程（2026-09-25）

`generate_storyboard` 保留为首次剪辑和改目标的默认流程。生成后对个别镜头不满意，走 `reselect_shots`（指定拍 P2→P5）或 `refine_shot_ranges`（只 P4→P5），其余镜头冻结，不重跑 P1。拍时长以当前时间线为准，配音不动。每次局部编辑写派生 storyboard + 新时间线，同事务提交；P5 校验不是可选步骤。工具按编辑意图命名，不按 Phase 暴露。

## 分析失败自动补跑（2026-09-24）

画面识别瞬时失败自动重新入队，每条最多 3 次；技术超时最多 2 次。最后不足 6 段也送审。用户跳过、不适用和 4xx（除 429）不补。不自动换 Provider。

## 输出端口可选编辑器（2026-09-18）

输出编辑器是项目级选择，记在 `settings_json.outputEditor`。未选择时按本机检测给默认值：CapCut 优先，只检测到剪映时用剪映，都没有时仍为 CapCut；默认值不写回设置。当前可交付：剪映（投放草稿）、CapCut（投放草稿）、FCPXML（Resolve / Final Cut 导入；Premiere Pro 只能导入 FCP 7 XML，不在支持范围）、OTIO（Resolve 导入）。文件导出不弹窗，写到本机 `editor-handoffs`。失败可见，不静默换端口。CapCut / 剪映草稿库都从该设备 `%LOCALAPPDATA%` 注册表识别，不写死盘符。

## 剪映是编辑器链接器（2026-09-18）

内部时间线是唯一剪辑事实。交付前投影为 `HandoffPlan`，由链接器写成目标编辑器格式。当前可交付剪映草稿、CapCut 草稿与 FCPXML/OTIO 文件。只新建、不覆盖、不回读。不把 OTIO 当成产品模型。链接器能力对不上就拒绝或保持未交付，不静默降级。

## 源窗短于口播时放慢（2026-09-18）

成片时长跟口播/节奏时钟。选中片段的运动可用窗不够长时，保留该画面并放慢播放，不换成更长的片，不冻帧，不补第二镜，也不拼下一段硬切。预览和剪映草稿都按源窗对槽位变速。

## 卡片让位于网格与 CLIP（2026-09-18）

不重跑第一次视觉分析。CLIP 可用时，没有片段图向量的候选不能靠卡片文字抬进前 9。Phase 3 以网格为准。整片候选 Phase 4 锁在 P3 源窗，不再 Pass A 另切窗。

## Phase 3 按可见证据选片（2026-09-17）

选片以该拍 `requiredVisual` 与网格画面为准；卡片文字是未核验标签，与画面冲突时看图。旁白只约束时长和口播，不单独决定选哪条。对不上直证时允许诚实的情景承载。网格按该条候选时间窗现拼，缺文件则从源片现抽。无图不作为硬失败。不按题材或画面类型设禁选清单。

## 瘦安装包与运行时模型下载（2026-09-19 修订）

默认安装包不捆绑 BGE/CLIP 的 `model.onnx` 大文件。启动缺权重时后台下载到 `%APPDATA%/<app>/runtime-models/`，SHA-256 校验后加载；tokenizer/config 仍随包。下载不阻塞工作台；缺失时 BGE 降级词面、CLIP 加权为 0。

下载策略：先 HuggingFace 官方，再国内镜像 `hf-mirror.com`（同路径同哈希，不是换模型）；单次尝试超时约 20 分钟，传输中断保留 `.partial` 并自动续传重试。发行方可选用完整包（`npm run models:fetch` 后 `npm run tauri:build:full`）把 ONNX 打进安装包，用户可跳过首次下载。FFmpeg/FFprobe、Python 与 Tesseract 的随包决策见以下各节。

## FFmpeg/FFprobe 随安装包（2026-09-20）

生产安装包捆绑 Gyan `ffmpeg-8.1.2-full_build` 的 `ffmpeg.exe` / `ffprobe.exe`（含 libass，供 preview 字幕）。调用一律走 `process::hidden_command`：优先 `FFMPEG_PATH`/`FFPROBE_PATH`，其次安装包或开发目录资源，最后系统 PATH。二进制 gitignored，构建前 `npm run ffmpeg:fetch` 或 `npm run tauri:build` 自动拉取。

## Tesseract 随安装包（2026-09-23）

生产安装包捆绑 UB Mannheim Tesseract 5.4.0 Windows 运行时与英文 `eng.traineddata`。构建脚本固定安装器与 7-Zip 解包器版本并校验 SHA-256，只解包到 `src-tauri/resources/tesseract`，不安装系统程序；二进制 gitignored。运行时解析顺序为 `TESSERACT_PATH` → 安装包/开发目录资源 → `Program Files` → PATH。`get_release_readiness` 通过 `--list-langs` 确认 `eng` 可用，缺失时阻止显示 ready。

## Python/剪映草稿 SDK 随安装包（2026-09-20）

生产安装包捆绑官方 Windows embeddable CPython 3.12.10，并预装 `pyJianYingDraft==0.3.0`、`pycapcut==0.0.3` 与 `MediaInfo.dll`。`python_program()` 优先 `PYTHON_PATH` 与随包 `python.exe`，未拉取时 Windows 回退 `py`，且不得把 `py -3` 传给 embeddable 解释器。启动适配器时把随包 FFmpeg/Python 目录插到子进程 PATH 前面。运行时 gitignored，构建前 `npm run python:fetch` 或 `npm run tauri:build` 自动拉取。

Tauri CLI 多次 `--config` 会覆盖 `bundle.resources`，不会拼接数组。`scripts/run-tauri.mjs` 在构建时把 `tauri.conf.json` 基础资源与 FFmpeg/Python（及可选完整 ONNX）清单写成一份 `src-tauri/target/tauri.merged-resources.conf.json` 再交给 CLI。Python 树用目录路径拷贝，不用 glob。

## 默认直推 master（2026-09-16）

单 Agent 默认在 `master` 提交并推送。不为每个改动建分支、开 PR，也不在每次提交前跑完整 `harness:test` / `cargo test` / `npm run build`。多个 Agent 并行时才用独立分支和 worktree。提交钩子只拒绝 detached HEAD，并检查文档同步。

## 全局共享子素材库（2026-09-16）

素材按导入根目录归入全局子库，项目创建时默认选中当时全部已有库，之后新建的其他库不会被自动加入。关联复用素材 ID 和分析结果，不复制媒体；成员身份不依赖重链路后的路径。旧项目保留自己的素材，主动导入的库自动用于当前项目。浏览、搜索、选片与输出使用同一共享范围；已有收藏、排除及分析结果随同一素材共享。本轮不加入额外画幅、时长或模板设置。

## 一体化窗口顶栏（2026-09-15）

窗口外观：系统标题栏改为现有工作台顶栏内的最小化、最大化/还原、关闭按钮，不新增一行；窗口拖动使用 Tauri drag region，顶栏改为与内容一致的暖白底色。

## 对话媒体选择（2026-09-15）

对话区配音、字幕、BGM 是独立的本轮自动添加选项，默认全开，选中显示勾选标志，发送后保存快照。同一会话恢复最近已发送选择，未发送修改仅保留在内存。明确文字要求优先于开关默认；关闭不删除既有轨道。`scriptMode` 描述稿件形态，不再单独决定是否调用配音；是否添加配音、字幕由媒体选项控制。配音开着必须配音：已有可念稿则照念；只有主题时 Agent 先写稿问同意，同意后再生成并合成。第一版只提供开关，音色、字幕模式和音乐风格下拉不在本轮范围。

## 1. 本地优先

项目数据、原始媒体引用、分析结果、storyboard、内部 timeline 和 preview 默认保存在本机。导入不会修改原始媒体。

## 2. Rust 负责本地副作用

React 通过 Tauri 命令表达意图，Rust 负责 SQLite、文件、媒体工具、模型请求和产物创建。前端不直接访问数据库或本地文件。

## 3. Agent 使用工具完成剪辑

自然语言请求进入 Agent 工具循环。模型可以观察项目状态、选择素材、生成 storyboard、编辑 timeline 和请求 preview；模型文字本身不能证明产物存在。

## 4. Storyboard 先于 Timeline

Storyboard 必须基于已分析的真实素材证据，并记录素材 ID 和源时间范围。内部 timeline 从已验证的 storyboard 创建，不能凭文件名或模型自述猜测媒体内容。

## 5. 产物使用新版本

Storyboard、timeline、preview 和编辑器交付物不覆盖旧版本。链接器产物是从内部 timeline 创建的单向交付物，不反向同步。

## 6. Provider 可替换

模型连接通过统一 Provider 入口，支持 OAuth 和自定义 OpenAI 兼容 API。凭据保存在 Windows Credential Manager，不写入 SQLite、浏览器存储或普通日志。

## 7. 失败要可见

Provider、媒体工具或数据库失败时返回真实且可理解的错误。不要用假成功或静默 fallback 掩盖错误。重试只在确实能改善临时网络问题、且不会重复本地副作用时使用。

## 8. 用户确认不可逆操作

最终导出、覆盖已有导出、删除项目或素材等不可逆操作必须先获得明确确认。当前剪映链接器只生成新的草稿目录。

## 9. Preview 是本地检查产物

Preview 使用本地 FFmpeg 生成，用于检查节奏、字幕和画面。Preview 不是最终导出，也不会修改原始媒体。

## 10. 低成本优先

早期产品优先简单的本地分析与可解释排序。片段级视觉证据采用**按需 + 永久缓存**：本地场景分段整库后台补跑（不花钱），视觉模型只对进入粗召回的素材调用。遇到质量问题，先增加复现和测试，再决定是否扩大模型调用。

## 11. 片段级分析取代固定 4 帧（2026-09-09）

技术分析以 FFmpeg 低分辨率场景检测硬切，CLIP 验真后写入真实片段，再在片段内用帧差运动能量收缩可用窗（`analysisVersion=4`）；无已验证硬切则整条一段，曲线对比不够则不收缩。选镜候选单位为片段，Phase 4 窗口用可用区间。Phase 1 在拆 beat 前注入本地库视觉/OCR 库存摘要，约束 `requiredVisual`/`visualKeywords` 贴近库内真实画面，禁止编造库存没有的主体；brief 叙事意图仍优先。配音开启时先按已确认 brief 合成旁白再拆拍；合成失败不生成。拆拍时写入预计拍数；平均一拍明显长于约 4 秒则软反馈再拆一次，最后一次仍粗则收下。用户给了成片秒数且与口播相差超过约 30% 时先问用户，并给出按实测语速换算的目标稿长；生成暂停等用户决定时，稿子存为任务 brief，快照写明暂停于哪个工具，下一轮按用户回答续跑。时间戳优先词级 alignment，没有则句内按字数插值。Phase 2 每个 beat 从全库硬切段里取 9 条（同片最多 2 段，相似最多 2 条；无硬切则整条 1 段）。有 1 条就能覆盖该 beat。后面 beat 不因前面池子里没用上的相似段被预删。Phase 3 每一拍单独看池内每条候选的图（弱匹配扩池到 12 条时也全附；按该条时间窗现拼网格），返回 `assetId+segmentId`，同一 beat 禁止同片；跨 beat 允许同一素材的不同、不重叠、不相似片段。选片以 `requiredVisual` 与网格为准（卡片与画面冲突时看图），旁白不单独决定选片。覆盖 beat 允许 1 镜；有第二条不相似且对得上才加到 2–3。主选和替补只允许前 5 条。整条选不出镜头则问用户。已用片段的相似画面硬拒。没有场景段、或段上没卡但有旧整片卡时按整条参与。Phase 2 评分另含 CLIP 图文权重（beat 文案 ↔ 片段代表帧），与 bge/词面并存；模型缺失时该分为 0；有 CLIP 查询但候选没有片段图向量时，词面/语义只当弱提示。导入后粗视觉按硬切段各送中点 1 帧、最多 6 段一批；第一次识别写模型自拟的叙事功能短语和可见描述，并写入片段向量，不抬 `visualAnalysisVersion`，不把叙事功能收成枚举。选片不等待第一次视觉分析，也不等待片段模型加深；召回使用已打上第一次卡的段，段上没卡但素材级有旧整片卡时按整条进池，未打卡段不借用整片标签。配音已把短 brief 锁成 `full_script` 时不再用「必须 key_message」打回。关配音不再强制 15 秒；时长由模型按用户要求或内容决定，没说时长时建议 15–45 秒，每个镜头大约 2–3 秒。配音开着必须配音；没有可念稿时 Agent 先写稿问同意，同意前不生成。第二次分析是选中镜头的 Phase 4 多帧精修：窗口锁在该片段，不因时长不够拼下一段硬切。Phase 3 选出后若可用窗短于该拍旁白，保留已选镜头并放慢，不换片、不问用户、不问 Phase 4 去拉窗。故事版落库后同一调用自动预览并新建剪映草稿；能播就不因缺口推迟预览。已有配音后禁止改旁白。工作台可列出并打开故事版版本；Agent 改当前打开的那一版，再次生成仍新建。

## 12. key_message 只出字幕标记不配音（2026-09-08）

`key_message` 是短目标/提纲成片：Phase 1 默认不写 `onScreenText`，除非用户明确要求屏幕字；`narration` 留空。镜头时长按目标时长均分到各拍（无标记时每拍约 2 秒底）。时间线**不自动配音**。显式 `synthesize_voiceover` 仍可用，但不得朗读 `onScreenText`，仅在 beats 仍有 narration 时合成。`full_script` 继续走口播 + audio-first + 自动配音。

## 13. scriptMode 由系统在 Phase 1 前锁定（2026-09-08）

`scriptMode` 是旁白/字幕产品路径的开关，不得交给模型自选。配音关闭时，Rust 用 brief 朗读估算（≥约 20s → `full_script`，否则 `key_message`）写入 Phase 1 必选约束。配音开启且已有可念稿时锁定 `full_script` 照念；没有可念稿时不生成，由 Agent 写稿并问用户同意。响应后再强制钉死。模型只负责在锁定模式下拆 beat 结构。

## 14. 界面中英双语（2026-09-25）

一个安装包、应用内切换语言，不分中英两套构建。界面文案集中在 `src/lib/i18n`：中文词典为基准，英文词典按其类型声明，漏键即编译失败；带数量或参数的文案写成函数，英文在函数里处理单复数。语言偏好只是界面便利，存本机浏览器存储；首次启动按系统语言（`zh*` → 中文，其余英文）。界面语言不等于成片语言：配音、字幕与分镜文案跟随用户文案或项目设置，不跟随界面。新建项目、会话的默认名称和欢迎语按创建当时的界面语言写入，之后不随切换改名。Rust 面向界面的状态文案给稳定键 `messageKey` 与参数 `messageParams`，由前端按界面语言翻译，中文原文保留作未知键回落与日志；能由已有字段推出的文案（编辑器名称按 `id`、交付结果按 `editorId/status/displayName`、连接状态按 `state`）直接在前端拼出，不新增字段。发送消息时附带界面语言，存入 Agent 任务输入，只决定 Agent 回复与系统兜底文案的语言。前端默认会话名是 Rust 识别的占位标题，两处必须同步。

## 15. 当前未完成事项

- FFmpeg/FFprobe、Python/剪映草稿 SDK 与 Tesseract/英文数据已随安装包。
- 最终视频导出尚未实现。
- 多轨媒体能力仍在迭代。
- 官方模型 OAuth 契约和部分外部 Provider 能力仍需真实环境验证。
- 浅色工作区替换面板尚未适配片段候选卡（`shot_replacement.rs`）。
