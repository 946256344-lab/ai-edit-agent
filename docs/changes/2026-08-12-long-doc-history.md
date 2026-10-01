# 长期文档历史补充

原实现与验证记录按当时文档保留，不代表当前契约。

## 原长期文档补充：本地音乐轨（2026-08-12）

来源：`docs/architecture.md`，文档整理前的历史表述；现状以长期文档为准。

版本化 `TimelineContent` 增加 `musicTracks`。每个 cue 绑定已分析的本地音频素材和明确源/时间线范围，可设置循环、音量与淡入淡出。preview 通过 FFmpeg 本地处理和混音，源媒体保持不变。Jianying 适配器现在通过本机 `pyJianYingDraft` 的 `AudioMaterial`/`AudioSegment` 创建独立音频轨，并映射源范围、循环拆段、音量和首尾淡入淡出；已用合成素材创建并注册新的草稿、检查到 1 条音频轨、1 个素材和 3 个循环片段。该结果尚未在 Jianying UI 中试听，所有音乐 draft 均为实验性且需要用户复核，绝不覆盖既有 draft。

Jamendo 是首个可替换线上音乐 Provider。其 `client_id` 仅存 Windows Credential Manager；`search_music` 仅返回 API 明示可下载且为 CC0/CC-BY 的曲目，CC-BY 的曲名、作者和许可 URL 会随 music cue 保存。`download_music` 才按需将单曲写入当前 local project 并交给既有本地分析队列；`use_online_music` 在一个具名、受限且可审计的调用内下载一首、等待分析完成并新建含循环背景音乐的时间线版本。每个下载副本使用唯一文件名，绝不覆盖既有本地副本。不会抓取网页、批量缓存曲库或把未验证的远程 URL 写入时间线/Jianying draft。

场景检测：一律先扫关键帧。全帧补扫仅用于本机、≤60 秒、且关键帧切点 ≤1 的片子；共享盘、长片或关键帧已切开则不再整段解码。FFmpeg `fps=3,scale=160` + `select=gt(scene,0.30)`。CLIP 对切点前后各一帧做相似度验真（阈值与 Phase 2 去似相同 0.92）；两侧仍像同一画面、抽帧失败或 CLIP 不可用则丢掉该切。无已验证硬切时整条一段，禁止按秒均分，不为凑数量把真切点合成 24 段。每段抽帧取首/中/尾并按约 4s 加密，上限 8 帧。硬切确定后对每段抽灰度序列算帧差能量，只收缩静止开头和已收敛结尾；对比不够、手持/流水线或抽帧失败则保持硬切两端。`TechnicalMetadata.analysisVersion=4`；version&lt;3 的就绪视频由 `reanalyze_asset_segments` 整段重切，version=3 只补运动曲线且不改视觉。保持 `ready`、不自动排队视觉。Phase 4 锁定片段时窗口用运动可用区间，曲线拿不准才标 `uncertain` 加密 Pass C。关键帧网格改为片段中点帧拼图。

生成 storyboard 前，brief 仅在本地与素材显示名、文件夹组织 hint 和 OCR 做词汇重合排序；只把纯数字 priority 写入 queued 视觉批次，相同分数按创建时间和任务 ID 稳定排序。生成不等待第一次视觉分析；召回使用已打上第一次卡的段，段上没卡但素材级有旧整片卡时按整条进池，未打卡段不借用整片标签。配音已把短 brief 锁成 `full_script` 时不再用「必须 key_message」打回。关配音不再强制 15 秒；时长由模型按用户要求或内容决定，没说时长时建议 15–45 秒，每个镜头大约 2–3 秒。配音开着必须配音；没有可念稿时 Agent 先写稿问同意，同意前不生成。文件名、文件夹和路径不进入 Provider；OCR 不进入粗视觉请求，但仍可作为明确标注的本地提取文字证据进入 storyboard，不能冒充画面语义。

**Storyboard 五阶段生成流程**：Phase 1 **先由 Rust 按 brief 朗读估算锁定 `scriptMode`**（≥约 20s 可念稿 → `full_script`，否则 `key_message`）。配音开启时先按已确认 brief 合成旁白，再用真实口播时长拆拍；合成失败不生成。用户给了成片秒数且与口播相差超过约 30% 时先问用户。再注入本地库视觉/OCR 库存摘要约束 `requiredVisual`/`visualKeywords`（禁止编造库中没有的主体；时长由模型按用户要求或内容决定，120 秒为安全上限；`key_message` 默认不写 `onScreenText`；不再用 8 秒或最少拍数硬打回；已知时长时写入预计拍数，平均一拍明显长于约 4 秒则软反馈再拆一次，最后一次仍粗则收下；每 beat 另产英文 `visualKeywords` 供本地召回）。Phase 2 **本地按段召回**：就绪视频全部展开为已打卡硬切段（无硬切、或段上没卡但有旧整片卡则整条 1 段）→ 补第一次段卡的本地片段向量（不等待模型加深）→ 每个 beat 取 9 段（同片最多 2，相似最多 2；有 1 条就能覆盖；后面 beat 不因前面池子里没用上的相似段被预删）。词面匹配不计英文虚词、只按词首命中，出现在多数拍查询里的通用词降权；CLIP 分按本拍全库余弦中位数到最高值相对换算。Phase 3 **各拍同时发请求**（上下文只含修复轮保留或局部重选冻结的镜头；返回后按拍序分配，撞上前面拍已选片段的拍剔除已用候选后再同时补发，直到没有撞车），看池内每条候选的网格（弱匹配扩池到 12 条时也全附；按该条候选时间窗现拼，缺文件则现抽），卡片含可见描述；用 `candidateIndexes` 默认选 **1 候选**（Rust 解析 asset/segment/源范围；序号必须 0–4；同一 beat 禁止同 `assetId`；跨 beat 允许同一素材的不同、不重叠、不相似片段，含相邻；已用片段相似画面硬拒，且进入选片前 Rust 已从池中剔除与已选镜头交叠/相似或素材复用已到上限的候选；撞车时只打回靠后的镜头；每镜约 2–3 秒，一拍一镜，两条不相似且对得上才加第 2 镜，不垫时长；40% 上限按素材）。选片先对网格画面（卡片文字与画面冲突时以图为准），旁白不单独决定选哪条。某一拍失败只重试该拍。整条选不出镜头则 `storyboard_needs_user_decision`。选出后 Rust 对照该拍 `narrationMs` 与候选 `usableMs`（运动可用窗）；不够长则保留已选镜头并放慢，不换片、不补第 2 镜、不拼下一段硬切。Phase 4 **是选中镜头的第二次视觉分析**：有片段锁定则跳过 Pass A，窗口=该片段的运动可用区间（无曲线则用硬切两端），不够长也不拼下一段硬切，也不再要求模型把窗拉过可用上限；网格多帧判断动作是否做完、后半截有无新信息并改 `sourceStart/End`。没有 segmentId 的整片也锁在 P3 源窗，不再 Pass A 另切窗。Phase 5 Rust `normalize` 自修后硬校验（保留 P1/audio-first 的 `targetDurationMs` 与 `scriptMode`；画面短于旁白不回 Phase 4 拉窗；精修类失败回 Phase 4 且只改受影响镜头，结构/硬上限与 diversity/相似片段失败不空转 Phase 4）。单步重试分离传输/语义预算；`previousShots` 只作提示快照，真实精修进度在 `Phase4Session`。耗尽错误含 `partialCandidateSummary`。收尾缺口走 `qualityWarnings` + `insert_clips`，禁止为补镜改 brief 重开；时间线有可播镜头时同一调用内自动预览并新建剪映草稿，缺口不挡预览。已有配音后禁止改旁白。实现位于 `src-tauri/src/storyboard/phases.rs`、`phase4.rs` 与 `step_retry.rs` / `provider_trace.rs`，主流程位于 `storyboard.rs::generate_storyboard_internal`。

storyboard 生成会记录详细日志：入口参数、素材库存、Phase 1 完成、Phase 2 每 beat 的 `poolSize`/`libraryExhausted`、Phase 3 `selected[assetIds]|uncovered`、Phase 4/5 attempt 与 issue kind、归一化与验证结果。debug 且 `STORYBOARD_PROVIDER_TRACE=1` 时另写 `src-tauri/target/storyboard-provider-trace.jsonl`（phase/attempt/direction/遮蔽 body）。模型传输复用进程级 `ureq::Agent`；自定义 API 可配置独立粗视觉 Model。

关键帧统一缩放到 320px 宽后计算拉普拉斯方差，取归一化中位数作为素材质量分；旧素材在首次 storyboard 前从既有关键帧补齐。视觉 evidence 写入后生成 512 维文本向量；向量与模型名、维度、版本、证据文本 SHA-256 一起保存在素材 `metadata_json`。旧素材按项目批量补齐，条件更新避免覆盖并发视觉分析；向量只供 Rust 排序，不进入 Provider payload。历史使用次数来自每个剪辑任务最新时间线，同一任务重复镜头只计一次。
