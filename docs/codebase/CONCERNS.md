# 代码库关注点

本页按当前源码列风险，不沿用旧扫描的行数、提交频率或已经完成的拆分计划。产品优先级由 TASKS.md / decisions.md 所有；本次静态核实不证明桌面可用。

## 优先风险

| 关注点 | 当前证据与影响 | 验收边界 |
|---|---|---|
| 删除/清缓存的前端确认 | App.tsx、ProjectSettingsModal、useNavigationEditController 仍使用 window.confirm；后端 confirmed=true 不能证明前端取得有效确认 | 已登记在 TASKS.md；必须实测取消不删除、确认才执行，本文不修业务 |
| 策划方向与实现阶段不同 | storyboard.rs 仍先按 brief/库存摘要写 narrative，再 P2→P3 选片；未实现体裁控制，库存提示不能保证逐段先引用镜头 | 不把「素材先行策划 + 体裁」方向宣传为已实现 |
| ASR 是占位 | subtitle.rs::transcribe_asset 按真实时长生成最多 40 条固定占位句、engine=local_stub_v1、confidence=0.55 | 不能把工具名/Schema 描述当真实语音识别；代码没有加载 whisper 模型自动升级路径 |
| 原生文字兼容判定不一致 | storyboard_text_tracks 可把带描边/阴影字幕标 verified，timeline::validate_text_tracks 对这类样式判 local_preview_only；jianying 依赖 cue 判定交付 | TASKS.md 已登记；需剪映/CapCut 实测并统一判定 |
| FCPXML 变速取窗待验证 | handoff/fcpxml.rs 的 timeMap/timept 与 asset-clip 源起点组合已有待查 | 用真实放慢镜头导入 Resolve/FCP 核对画面，写文件成功不证明语义正确 |
| 重复文件夹导入 | assets.rs::store_assets 每次新建 ID，不按源路径去重 | TASKS.md 已登记，不能把共享库复用描述为所有导入都会去重 |
| 发行/编辑器效果未闭环 | 四端口、旁白/品牌/转场有写入代码，但桌面/干净机/签名等证据独立于源码 | 以 release-checklist.md 的当前待确认项为准 |

## 当前结构与契约风险

- assets 已拆 library/analysis/health/visual/controls/progress/segments/motion/retry/beats；agentloop 已拆 native/tools/skills/snapshot/context/prompt 等。旧「仍混合全部职责、下一步提取 library/router/state」路线已过时；不要据此新开重构任务。
- 领域 SQL 仍分散，receipt 占用/消费、终态消息、局部故事版+时间线和审计依赖原子事务；搬迁不能拆断作用域与恢复行为。
- tools.rs、policy.rs、native 白名单、TS 工具镜像及版本化 fixture 是多个同步点。当前 33 工具一致，但手工维护仍有漂移风险；harness 的名称检查不证明领域契约和模型行为正确。
- fixture 文档仍标完整场景为 fixture_only，局部 scripted 响应测试不能替代所有场景在临时 SQLite 上运行的多轮 E2E；既有单测与工具契约检查各自只覆盖自己的边界。
- tsconfig.app.json 未开启 strict 总开关；局部严格选项/build 不能等同完整 strict。改动验证遵守 CONTRIBUTING.md，不由本页要求默认全量测试。
- local-store.ts 集中 invoke，module 扩大时要保留单一公开命令面；不能因 IDE 工具镜像存在就开放前端任意工具调用。

## 外部、安全与本地边界

| 边界 | 当前缓解 | 尚需注意 |
|---|---|---|
| Voycut 网关与凭据 | 内置网关构建不回退本机 Provider/配音 key；Windows Credential Manager 保存秘密 | 服务端资格/额度/服务商选择不由此仓库静态验证；get_voice_availability 探测失败不等于明确无能力 |
| 无网关开发连接 | custom_api 用户配置、实验性 OAuth；凭据读取失败封闭 | custom_api 输入校验主要为非空，HTTP/localhost 等 URL 策略仍需单独决定；不虚称 TASKS 已登记新的待决问题 |
| 无网关配音回退 | Fish 传输类失败可用已配置 ElevenLabs，认证失败不回退 | 与「失败不静默换 Provider」原则存在需主会话澄清的范围差异，网关模式无回退 |
| 路径/日志/媒体上传 | 库投影/快照不暴露源路径，read_logs 固定范围并遮蔽；release 关闭 debug 全量转储 | 媒体图片和文本仍发远端模型，不能写「不上传任何内容」；部分底层错误/log 包含路径，应持续审查 |
| 进程与截止时间 | process.rs 无窗口创建、超时、taskkill 进程树回收；同步 Agent 步骤共享截止时间 | ONNX/文件操作无法强制中断；独立 preview/适配器不是统一用户可取消作业，终止失败/孤儿进程仍需运行证据 |
| OCR / 模型资源 | Tesseract 随包 eng，ONNX 下载校验/续传，DirectML 失败回 CPU | eng 不识别中文招牌；缺 BGE/CLIP 会降级，启动状态不能冒充完整模型分析质量 |
| 编辑器能力降级 | FCPXML 基础 Title、OTIO marker，黑场转场说明；品牌图片文字不可编辑 | 不能把四端口都称为完整原生字幕/动态一致，普通图片主镜头与品牌 PNG 能力不同 |

CSP 仍允许 Google Fonts 域，但当前 index.css 未发现远端字体 import；不能沿用「正在发 Google Fonts 请求」的旧结论。是否有其他运行时网络字体请求本次未核实。

## 性能与恢复

技术并发为核数 1/4、夹到 2–8，视觉最多 16 个任务且同素材互斥；不再是旧「2 个技术 worker、1 个视觉 worker」。Phase 3 各拍并发，Phase 4 拆批并保存成功进度，BGE/CLIP 同模型推理串行、DirectML 优先。实际耗时取决于机器/素材/Provider，本次未重测。

素材刷新按活动分析/扫描和事件唤醒；不能沿用「始终每 1.5 秒全库轮询」说法。SQLite 每次打开仍迁移检查，高频查询/源文件访问及远端共享盘延迟要用实际证据评估。preview 分层哈希缓存有项目 2 GiB 上限，成功才复用；长片/多轨的磁盘和渲染耗时、全流程取消仍需实测。

## 文档与事实边界

长期文档只描述现状，历史进入 changes；decisions 是主会话所有的有效决策，执行会话发现偏差只回报。代码已实现、静态检查通过、真实桌面验收、发布完成是不同事实，不能互相替代。交接必须保留当前 Git 状态、任务窗口、提交号和验证结果。

## 证据

- TASKS.md、docs/decisions.md、docs/release-checklist.md（只读）
- src/App.tsx、src/components/ProjectSettingsModal.tsx、src/hooks/useNavigationEditController.ts
- src-tauri/src/subtitle.rs、storyboard.rs、timeline.rs、handoff/{mod,fcpxml}.rs
- src-tauri/src/agentloop/{tools,policy,native,schema,skills}.rs、src/lib/agent-tools.ts
- src-tauri/src/assets/{analysis,visual,segments,library,retry}.rs、assets.rs
- src-tauri/src/provider.rs、custom_api.rs、music_provider.rs、process.rs、runtime_models.rs、onnx_device.rs
- src-tauri/tests/fixtures/README.md、tsconfig.app.json、src/index.css、src-tauri/tauri.conf.json
