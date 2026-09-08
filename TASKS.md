# 任务清单

## 当前任务窗口

<!-- ACTIVE_TASKS_START -->
- [x] 完成（2026-09-08，fix/phase3-diversity-cap）：Phase 3 补 40% 复用上限 issue + prompt 数值；Phase 5 diversity/相邻同片失败不回 Phase 4。见 docs/changes/2026-09-08-phase3-diversity-cap.md。
- [x] 完成（2026-09-08，fix/phase4-overlap-resolve）：Phase 3 相邻同片硬拒（含跨 beat）；Phase 5 diversity 对齐。见 docs/changes/2026-09-08-phase3-consecutive-asset-ban.md。
- [x] 完成（2026-09-07，fix/phase4-overlap-resolve）：Phase 4 钳窗后窗内机械消交叠；normalize 保留消交叠但清被挪镜的 cropFocus，有构图不做整段 pack。见 docs/changes/2026-09-07-phase4-overlap-resolve.md。
- [x] 完成（2026-09-07，fix/phase4-refine-batch）：Phase 4 Pass B/C 每镜满采样拼网格并按最多 10 镜拆批，不降精修密度。见 docs/changes/2026-09-07-phase4-refine-batch.md。
- [x] 完成（2026-09-07，fix/voice-http-proxy）：配音 ureq 读取 HTTPS_PROXY；中文 Windows 10060 归类为 timed out；设置页区分已连接与密钥已存未探通。见 docs/changes/2026-09-07-voice-http-proxy.md。
- [ ] 后续：预览缓存 `previews/cache/<projectId>` 增加按项目清理或占用上限；合入 smooth-editing-pipeline 后另开分支，避免本机派生视频无限堆积。
- [ ] 后续：合入后用 1–2 个真实项目抽检竖屏 `cropFocus`（预览裁剪与 Jianying 草稿主体是否一致）。
- [x] 完成（2026-09-07，codex/smooth-editing-pipeline）：预览分层复用、空闲停止刷新、候选评分修正、配音/语义并行、句级对齐与镜头衔接/竖屏构图；健康摘要仅在计数/任务状态变化时 bump 素材页。见 `docs/changes/2026-09-07-smooth-editing-pipeline.md`。
- [x] 完成（2026-09-07）：完整文案由模型标 full_script+spokenScript；TTS 照念原文不用改写 beats。见 `docs/changes/2026-09-07-full-script-verbatim-voiceover.md`。
- [x] 完成（2026-09-07）：配音旁白必写（alignment 失败不挡写入）；Fish 传输类失败回退 ElevenLabs；快照 `voiceoverCues`/`配音能力` 与 `voiceoverApplied` 事实返回。见 `docs/changes/2026-09-07-voiceover-must-apply.md`。
- [x] 完成（2026-09-07）：短对齐字幕动画钳制；字幕校验失败不挡旁白写入。见 `docs/changes/2026-09-07-voiceover-short-subtitle-animation.md`。
- [x] 完成（2026-09-04）：可念稿强制 full_script+audio-first；normalize/join 去重旁白；key_message 旁白硬门；配音 fit 失败可见警告。见 `docs/changes/2026-09-04-voiceover-narration-contract.md`。
- [x] 完成（2026-09-04）：对话「处理中」改为可点「停止」；`cancel_agent_edit` + Cancelled 终态。见 `docs/changes/2026-09-04-cancel-agent-edit.md`。
- [x] 完成（2026-09-04）：侧栏剪辑会话右键删除；级联清除对话/Agent/故事板/时间线/本地 preview，素材保留。见 `docs/changes/2026-09-04-delete-editing-session.md`。
- [x] 完成（2026-09-04）：Phase 4 去掉每素材场景扫描，改用导入关键帧/三分段建粗窗；修 `clamp_shots_to_chosen_windows` 窗尾 panic。见 `docs/changes/2026-09-04-phase3-4-keyframe-inspect.md`。
- [x] 完成（2026-09-04，fix/no-keyword-agent-policy）：去掉 Agent 关键词判定行动（`只`/`only` 只读门与观察词表）；意图交模型，Rust 守白名单/作用域/领域校验。见 `docs/changes/2026-09-04-no-keyword-agent-policy.md`。
- [x] 完成（2026-09-04）：Phase 3 看关键帧选片；Phase 4 先选内容窗再段内精修（不确定加密 + 旁白时长托底）。见 `docs/changes/2026-09-04-phase3-4-keyframe-inspect.md`。
- [x] 完成（2026-09-04）：`key_message` 收敛为 ≤15s 短视频（默认 8–15s、2–5 beat；短 brief 硬帽 15s）。见 `docs/changes/2026-09-04-key-message-15s-cap.md`。
- [x] 完成（2026-09-04）：storyboard 完成后统一自动配音。有 `narrationText` 且语音 Provider 已配置时，Agent/前端共用 `auto_synthesize_storyboard_voiceover`；失败不挡预览；已有旁白轨跳过。见 `docs/changes/2026-09-04-auto-voiceover-after-storyboard.md`。
- [x] 完成（2026-09-03）：Storyboard 安全上限抬到 100 镜/beat；Phase1 短 brief 偏 key_message/短时长；Phase5 机械自修、结构问题不回 Phase4。见 `docs/changes/2026-09-03-storyboard-shot-cap-and-short-brief.md`。
- [x] 完成（2026-09-03，refactor/storyboard-select-then-refine）：Storyboard 单步重试原语 + Phase2 去重补位 Top12 → Phase3 选 2–3 → Phase4 时间段精修 → Phase5 校验；uncovered 闭环文案与 `partialCandidateSummary`；`STORYBOARD_PROVIDER_TRACE`。见 `docs/changes/2026-09-03-storyboard-select-then-refine.md`。
- [x] 完成（2026-09-03，fix/beat-min-shots-and-timeline-gate）：Phase 3 硬门禁「每个已覆盖 beat ≥2 镜」；generate_storyboard 收尾检查 uncovered / 镜数 / audio-first 缺口，经 qualityWarnings 触发精炼续步。见 `docs/changes/2026-09-03-beat-min-shots-and-timeline-gate.md`。
- [x] 完成（2026-09-03，feat/opencut-full-port）：音频优先 + 禁止冻结帧。`full_script` Phase1 后 TTS 定时长；画面不足不挂不匹配配音；新增 `insert_clips` 与 `voiceover_longer_than_picture` 可恢复失败上下文；契约/文档已同步。见 `docs/changes/2026-09-03-audio-first-no-freeze.md`。
- [x] 完成（2026-08-27，chore/cleanup-rust-warnings）：清理 Rust 后端 18 个 unused/dead-code 警告；删除旧 `request_storyboard` 与场景检测遗留代码，预留契约以最小 `#[allow(dead_code)]` 保留。262 个 Rust 库测试、agent/harness 检查通过；无命令、schema、工具白名单或 Provider 协议变化。见 `docs/changes/2026-08-27-cleanup-rust-warnings.md`。
- [x] 完成（2026-08-27，codex/dynamic-tool-loading）：NativeToolLoop 每轮提供完整工具名称/一句话目录，`load_tools(toolNames)` 每次替换并最多暴露 5 个完整 schema；加载从下一次 Provider 请求生效，Rust 拒绝同响应未暴露调用。`read_logs` 可由模型按需加载，按 1-based 行号范围读取当前活动应用日志，具备固定路径、分页、字符预算及敏感行遮蔽。会话历史取消固定条数/字符窗，完整 Provider payload 以 token 计量，40K 触发模型自主压缩、目标 30K、硬上限 60K。见 `docs/changes/2026-08-27-dynamic-tool-loading-and-log-reading.md`。
- [x] 完成（2026-08-27，codex/semantic-storyboard-retrieval）：Storyboard Phase 2 每 beat 以本地中文向量/词面降级召回 Top-12，模型选 1 个且 Phase 3 不得换出；接入真实关键帧质量、去重使用次数、旧素材安全回填和安装包内置模型。后续补齐同次 Storyboard 复用累计扣 15 分、连续复用额外扣 30 分，按实际已选镜头动态执行 40% 上限并硬排除相邻重复；Phase 3 固定已覆盖 rough shot 一对一顺序并保留 uncovered beat，避免多样性规则冲突。247 个 Rust 库测试、2 个契约测试、14 个 Python 测试及 agent/branch/harness 全绿；安装包验证沿用本分支此前通过的 release 构建，独立审查无剩余 P0-P2。见 `docs/changes/2026-08-27-storyboard-semantic-retrieval.md`。
- [x] 完成（2026-08-26，codex/video-only-storyboard-candidates）：Storyboard 每个 beat 的候选入口只接收 `analysis_status = 'ready'` 且 `kind = 'video'` 的未排除、可访问素材，图片、音频和其他类型不再补足 Top 5；回归覆盖六类资格边界，并记录 Rust 词面预排序与模型视觉复选的真实边界。233 个 Rust 库测试、2 个契约测试、14 个 Python 测试及 agent/branch/harness 全绿，独立审查无 P0-P2 问题。见 `docs/changes/2026-08-26-video-only-storyboard-candidates.md`。
- [x] 完成（2026-08-26，codex/relax-native-tool-policy）：NativeToolLoop 默认向模型开放可逆本地编辑工具，不再依赖“生成/制作”等正向关键词；Rust 仅收缩明确只读/禁止项，并继续负责敏感能力授权、参数与作用域校验、事务副作用和真实完成裁决。补充“剪辑一个视频”、中英文只读/否定语义及重复无效调用回归；232 个 Rust 库测试、2 个契约测试、14 个 Python 测试及 lint/build/agent/harness 全绿，独立审查无剩余阻断。见 `docs/changes/2026-08-26-relax-native-tool-policy.md`。
- [x] 完成（2026-08-26，codex/fix-agent-continuation-review）：NativeToolLoop 新增有界失败恢复与质量精炼续步；Bugbot 的字幕主题词误授权、旧 preview 收据及续步事实误清除问题已修复，且与权威状态快照完成集成。235 个 Rust 库测试、2 个契约测试、14 个 Python 测试及 lint/build/agent/harness 全绿。见 `docs/changes/2026-08-26-native-recovery-refinement-continuations.md`。
- [x] 完成（2026-08-25，codex/native-state-snapshot）：NativeToolLoop 每轮注入本地权威状态快照，写工具成功后刷新并受上下文裁剪保护；隐私/长度、preview 磁盘事实与观察门回归已覆盖，不改公开命令、工具目录或 SQLite schema。219 个 Rust 库测试、2 个契约测试、14 个 Python 测试及 lint/build/agent/harness 全绿，独立审查无剩余阻塞。见 `docs/changes/2026-08-24-native-state-snapshot.md`。
- [x] 完成（2026-08-20，feature/elevenlabs-voiceover）：「生成视频/配音」步骤耗尽修复。授权 storyboard+时间线+配音；有界 list_assets；空串当 null；Chat 工具消息合并；配音失败码。198 个库测试通过。见 `docs/changes/2026-08-20-elevenlabs-voiceover.md`。
<!-- ACTIVE_TASKS_END -->

## 说明

这里记录的是近期工作状态，不要求每个新任务都同步到长期文档。遇到问题时优先记录复现方式、实际错误和影响，再决定是修代码、补测试还是暂时接受限制。历史实现可以通过 Git 提交记录查看。
