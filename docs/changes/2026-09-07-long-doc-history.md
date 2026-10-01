# 长期文档历史补充

原实现与验证记录按当时文档保留，不代表当前契约。

## 原长期文档补充：Phase 4 精修拆批（2026-09-07，Pass A/长窗收窄 2026-09-08，局部修复 2026-09-10）

来源：`docs/architecture.md`，文档整理前的历史表述；现状以长期文档为准。

Pass A 按素材贪心拆批，每批最多 4 张窗中点帧；Pass B/C 保持每镜固定时间采样密度，将同镜多帧拼成网格后按最多 4 镜一批调用模型、各批并发（Agnes Token Plan 单请求最多 4 张图）。同镜窗内多帧优先一次 FFmpeg（`-ss` 窗首 + `select` 命中各目标时刻，文件名仍带真实 `time_ms`），批量失败再按帧回退。批结果必须完整覆盖本批 `orderIndex` 后才合并；Pass B/C 改为只返回本批修改，不再要求整份 Storyboard JSON。过短导入头窗（&lt;1.2s）向前并入后窗。Pass B 后若窗内帧间距 &gt;1.5s，Pass C 围绕精修子区间再加密收窄（uncertain 仍整窗加密）。重试从失败批次或受影响镜头继续，成功结果留在当次内存状态。Phase 3 每一拍为池内每条候选附带网格（弱匹配扩池到 12 条时也全附；按该条时间窗现拼，缺文件则现抽），卡片含可见描述并标 `keyframeGridAttached`。

## 原长期文档补充：配音出站代理（2026-09-07）

来源：`docs/architecture.md`，文档整理前的历史表述；现状以长期文档为准。

配音 HTTP 不走系统 WinINET 自动代理配置，只读进程环境中的 `HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY`。本机若只能经本地代理访问 `api.fish.audio`，旧版裸 `ureq` 直连会稳定超时；模型自定义 API 仍用独立 Agent，不受本次改动强制代理。

## 原长期文档补充：剪辑流程优化（2026-09-07，配音先于拆拍 2026-09-17）

来源：`docs/architecture.md`，文档整理前的历史表述；现状以长期文档为准。

配音开启时先按已确认旁白稿合成，再拆拍；语义/CLIP 编码在 Phase 1 之后、选片之前进行。Phase 2 独立建立各 beat 候选池，不把候选当作已使用镜头提前扣分，时长项按可用源窗是否容纳目标镜头评分。Phase 3 在原有一次请求里结合整条序列判断景别、完整动作、方向与首尾表达；Phase 4 附带真实配音时段并判断 `cropFocus`，裁剪焦点随分镜进入时间线、预览和 Jianying handoff。带焦点的镜头禁止整段 pack 到未检查源窗；Phase 4 钳窗后先在已选内容窗内机械消交叠，normalize 仍消交叠但仅清掉被挪源范围的 `cropFocus`。窗内仍无法拆开时才交校验回 Phase 4。

预览仍来自持久化时间线，新增 `preview_cache.rs` 管理跨版本复用键，镜头、无字幕底片、文字/叠加画面三层缓存留在本地。只有成功生成的临时视频才进入缓存；源大小或修改时间、源范围、裁剪变化会失效对应层。文字和叠加画面一次合成，音轨单独混合，桌面命令在工作线程执行。`useAssetWorkspaceController` 仅在分析/扫描活动期间继续刷新，由动作和队列交接事件唤醒；返回的 `visualPending` 覆盖当前页之外的视觉分析。健康摘要轮询仅在计数或任务状态变化时连带刷新素材页。

## 原长期文档补充：配音（Fish Audio / ElevenLabs，2026-09-07）

来源：`docs/architecture.md`，文档整理前的历史表述；现状以长期文档为准。

分镜完成后，若 storyboard 为 `full_script`、有旁白、语音 Provider 已配置、且时间线尚无旁白轨，则 **自动合成配音**：Agent `generate_storyboard` 调用 `auto_synthesize_storyboard_voiceover`（文本优先 beats）。**`key_message` 不自动配音**，默认不写屏幕标记字幕。合成失败只提示，不挡预览；`voiceover_longer_than_picture` 写入 `qualityWarnings`；已有旁白轨则跳过。有大段可朗读文案时 Phase1 **之前**由系统锁定 `full_script`（模型不得改选），并走 audio-first：配音开启时先按已确认 brief 合成，失败则不生成；时间戳优先词级，没有则句内按字数插值。其后自动配音因已有轨而跳过。

显式 `synthesize_voiceover` / `synthesize_storyboard_voiceover` 仍可用。文案只来自请求 `text` 或 storyboard `narrationText`，不得把 `onScreenText` 当旁白。密钥在 Credential Manager。**优先 Fish Audio**；传输不可用/超时/5xx/429 且 ElevenLabs 已配置时可回退（401/密钥错误不回退）。配音时长是时钟：旁白 cue 等于完整音频，画面不得短于口播（禁止冻帧垫片）。**旁白轨必写**；字幕只使用 TTS alignment 且尽力提交，失败保留旁白并记 warning。快照用 `voiceoverCues` 与 `配音能力` 区分「已写入」与「已配置」。preview 把旁白与可选 BGM 混到画面时长，禁止 `-shortest`。
