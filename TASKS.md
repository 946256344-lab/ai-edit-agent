# 任务清单

## 当前任务窗口

- [ ] 进行中（2026-09-26）：10 月 1 日首发准备，已定海外优先。待拍板：发布形态、试用额度与付费、上游模型成本、配音、交付物口径。P0 阻断项：正式构建注入网关地址、网站仓库提交与生产部署、网关额度与上游错误透传、经网关的 413 实测、默认编辑器改 CapCut 并实测 CapCut 与 Resolve 交付、网关错误文案英文化、英文界面主路径验收、代码签名、英文 Terms/Privacy 与删除渠道、第三方许可、隐藏实验性 OAuth。逐项见 docs/release-checklist.md。
- [ ] implemented_unverified（2026-09-26，claude/product-naming-voycut-3663a1）：故事版版本下拉移到预览面板标题行右侧；标题下只留时长与镜头数（去掉时间线版本号）；预览状态不再显示「预览已就绪」等平稳状态。tsc、lint、i18n 通过；待桌面确认。见 docs/changes/2026-09-26-cut-heading-meta-row.md。
- [ ] implemented_unverified（2026-09-25，claude/p2-p3-quota-allocation-939155）：Phase 3 网格上限跟候选池走。弱匹配扩池到 12 条时，后 3 条过去只有文字卡；现在池内每条都附网格，上限等于 Phase 2 最大池，提示词写实际附图数。cargo check 通过；待真实弱匹配回合验收。见 docs/changes/2026-09-25-phase3-grid-follows-pool.md。
- [ ] implemented_unverified（2026-09-26，claude/send-error-real-cause）：发送失败显示真实原因。任务归属的模型请求失败带稳定码前缀（`provider_timeout` 等），前端 `describeSendError` 按码给出超时、连不上、凭据被拒、限流、服务端错误等具体提示，兜底附脱敏原因摘录，不再只说「无法准备当前剪辑任务」。cargo check、lint、tsc、i18n 通过；待桌面实测。见 docs/changes/2026-09-26-send-error-real-cause.md。
- [ ] implemented_unverified（2026-09-25，claude/lens-p1-p5-selection-89a5aa）：选镜局部编辑工具。新增 `reselect_shots`（指定 beat 走 P2→P5，默认排除当前素材）与 `refine_shot_ranges`（只走 P4→P5），其余镜头、配音、字幕冻结，拍时长取当前时间线，同事务写派生 storyboard + 新 timeline 并出预览；`generate_storyboard` 保留为默认流程。cargo check、storyboard/tools 测试通过；真实桌面已验「换第 3 个镜头」与「精修第 2 个镜头切点」：只目标镜头变化，其余 6 镜、配音、字幕、音乐与总时长 18081ms 逐字不变。两拍同时重选、按素材名替换与派生版本标注待模型服务恢复后验收。前端派生版本标注、手动换镜召回升级另开任务。见 docs/changes/2026-09-25-storyboard-edit-primitives.md。
- [ ] 待修（2026-09-25）：Phase 3 增量重跑从未生效。`phase3_select` 无修复包时 `beats_named_in_repair` 返回全部拍，`prior_shots` 复用分支走不到；有修复包时又不加载 `prior_shots`。需决定是修复还是删除该路径（局部编辑已由 `reselect_shots` 承担）。
- [ ] implemented_unverified（2026-09-26，claude/product-naming-voycut-3663a1）：预览面板镜头条默认收起，舞台占满高度；舞台下一行放镜头计数（点击展开镜头条）与替换、撤销、重做、更新预览图标。tsc、lint、i18n 通过，临时页面核对 330px 宽布局；待真实桌面验收。见 docs/changes/2026-09-26-preview-collapsed-shot-strip.md。
- [ ] implemented_unverified（2026-09-25，claude/product-naming-voycut-3663a1）：界面改为 Voycut 品牌风格。靛紫主色与品牌渐变、冷灰画布上的白色面板、主操作改靛紫、选中态淡紫底或紫色光晕、侧栏与助手头像用 `BrandMark` 占位标志、素材库入口显示素材数。tsc、lint、i18n、文档同步通过，开发版截图核对主要页面与两种窗口尺寸；待正式图标与真实回合（处理中、镜头替换）验收。见 docs/changes/2026-09-25-voycut-visual-style.md。
- [ ] implemented_unverified（2026-09-25，claude/keen-driscoll-de8c99）：故事版切换显示派生版来源。`StoryboardVersion` 加可选 `derivedFromVersionId` / `changedBeatIds`，下拉项显示「v5（改自 v4，第 3 拍）」；缺字段按普通版本显示。选择器移入 `StoryboardVersionPicker` 组件。lint、tsc（本次文件）、i18n、文档同步检查通过；后端 `reselect_shots` / `refine_shot_ranges` 落地后待真实派生版验收。见 docs/changes/2026-09-25-derived-storyboard-version-label.md。
- [ ] implemented_unverified（2026-09-25，claude/laughing-stonebraker-eeb90a）：替换镜头「重新推荐」的 Phase 2 召回补齐语义/CLIP 向量、素材使用次数与按当前时间线的每拍时长，并为池内素材补 CLIP 图像向量；编码失败降级不挡推荐。cargo check 通过；待真实桌面验收排序效果。见 docs/changes/2026-09-25-shot-replacement-recall-signals.md。
- [ ] 部分已验证（2026-09-25，codex/fellowcut-auth-trial）：桌面模型请求接入 FellowCut 网关；公开构建无网关时失败封闭。合入最新 master 后真实 Tauri 开发版经受保护的 Vercel 预览网关得到 HTTP 200 与模型回复；修复工具白名单遗漏后，只读请求显示「完成」。本机已分析素材生成 3 镜头、约 8 秒粗剪预览，换镜保存为第 2 版并重新生成可播放 MP4；最终模型说明请求失败使粗剪回合仍标「部分完成」。素材导入、图像批量请求、剪映交付及正式安装包仍待验收。见 `docs/changes/2026-09-25-fellowcut-model-gateway.md`。
- [ ] 待对齐（2026-09-25）：素材库显示 95 条可见素材，Agent 健康摘要统计 102 条可访问资产，其中 7 条已标记从素材库移除；健康摘要需沿用素材库的过滤口径，避免模型把历史移除素材计入当前项目。
- [ ] 部分已验证（2026-09-25，codex/fellowcut-auth-trial）：桌面端接入 FellowCut 邮箱密码登录与 Firestore 试用资格只读显示。真实开发版打开了旧本机项目和剪辑会话；用户提供截图显示测试账号已登录、邮箱已验证、试用中，有效期为 2026-10-02 17:31:35。关闭后重新启动，顶栏恢复显示同一测试账号及旧项目；退出登录和安装包仍待验收。模型额度与付费控制留待服务端网关实施。
- [ ] implemented_unverified（2026-09-25，claude/frontend-ui-ux-optimization-1c61c6）：前端精修浅色改版。按确认设计稿改为苹果白与系统字体，样式收进 `src/index.css` 设计变量与 `src/styles/`，删掉 `App.css` 旧深色规则；会话改单色竖屏图标，处理进度默认收成一行，预览改浅色底与按镜头分段的进度条、6 格镜头条。开发版截图核对对话、预览、素材库、设置弹窗与三种窗口尺寸；处理中动效与镜头替换面板待真实回合验收。见 docs/changes/2026-09-25-refined-light-ui.md。
- [ ] implemented_unverified（2026-09-25，claude/frontend-ui-ux-optimization-1c61c6）：助手消息按 Markdown 渲染（`react-markdown` + `remark-gfm`），保留单个换行；不执行原始 HTML、不加载远程图片，外部链接交给系统浏览器。仿真回复渲染与链接不跳转已核对；真实桌面点链接待验收。见 docs/changes/2026-09-25-message-markdown.md。
- [ ] implemented_unverified（2026-09-25，claude/bilingual-chinese-english-c8ed75）：中英双语第二步。启动检查、模型下载、交付结果、编辑器名称、连接状态、候选不可用原因随界面语言显示；Rust 给 `messageKey`/`messageParams`，中文原文作回落。发送时带 `uiLocale`，Agent 回复与系统兜底文案跟随界面语言，成片语言不变。修复英文默认会话名不被首条消息改名。cargo check、新回归测试、tsc、lint、harness 通过；待真实桌面验收。见 docs/changes/2026-09-25-backend-text-i18n.md。
- [ ] implemented_unverified（2026-09-25，claude/bilingual-chinese-english-c8ed75）：中英双语第一步。前端文案收进 `src/lib/i18n` 类型化词典（英文漏键即编译失败），侧栏切换语言，偏好存本机，首次按系统语言；时间与排序跟随语言；`harness:check` 拦截组件里新写死的中文。tsc、lint、harness 通过，浏览器模拟 IPC 下中英切换与主要页面已看过；待真实桌面验收。见 docs/changes/2026-09-25-ui-i18n.md。
- [ ] implemented_unverified（2026-09-25）：修复启动卡顿与输入延迟。同步 Tauri 命令全部改为 `#[tauri::command(async)]` 移出 UI 主线程；模型权重校验按大小+修改时间缓存，不再每次启动整读约 2 GB 哈希；数据库迁移每进程只跑一次，不再每次打开连接争写锁；发送后立即显示用户消息并清空输入框，不再等任务归属模型返回。`cargo check` 通过；待桌面重建后验收。见 docs/changes/2026-09-25-startup-input-lag.md。
- [ ] implemented_unverified（2026-09-25）：剪映/CapCut 草稿交付补上配音轨。两者 `voiceover` 能力改为 Full，交付前解析配音来源，适配器新增 `add_voiceover_tracks` 写入独立音频轨 `assembly-voiceover-N`（不循环、源短于时间线取较短）。适配器 20 项、handoff 5 项 Rust 测试通过，真实最小输入生成含视频+配音轨的草稿；待桌面重建后用真实带配音项目验收。
- [ ] implemented_unverified（2026-09-25）：修复剪映草稿交付失败。根因是随包 pyJianYingDraft 0.3.0 把 `ScriptFile.add_track` 改成 `append_track(TrackSpec)`，适配器仍按旧 API 调用，立即抛 AttributeError；pycapcut 0.0.3 仍是旧 API，故在适配器加兼容层。同时日志带出适配器真实原因，前端交付错误显示具体原因，选剪映时检测草稿库并提示连接。适配器 19 项测试、lint、cargo check、harness 通过，真实最小输入已生成含视频+字幕轨的草稿；待桌面重建后走真实交付验收。
<!-- ACTIVE_TASKS_START -->
- [ ] implemented_unverified（2026-09-25）：Phase 3 增量重跑。每次生成分镜时，Phase 1 完成后从 DB 加载上一版 storyboard 的 shots，构造 `prior_shots_to_selections` 映射；beat id 未变且上次选中的 assetId/segmentId 仍在新候选池里的 beat 直接复用结果，跳过该 beat 的模型调用。修复轮（repair 不为 None）的受影响 beat 始终重跑。编译与单元测试通过；待真实多次迭代回合验收节省效果。
- [ ] implemented_unverified（2026-09-25）：五处 storyboard pipeline 智能化改进：①`StoryboardBeat` 新增 per-beat `paceHint`（short/default/long），Phase 1 模型生成叙事时按 beat 写节奏，Phase 3 优先使用 per-beat 值覆盖全局 `shotLengthHint`；②Phase 2 低分自适应扩池，最高分低于阈值时静默扩展到 12 条候选；③Phase 3 已选镜头上下文携带 beat purpose，帮助模型判断叙事连贯性；④Phase 3 低分池预警，明确告知模型库存匹配度有限；⑤Phase 4 Pass C 早停，全部 Pass B 镜头收敛时跳过 Pass C 省去冗余精修。编译通过，既有单元测试全绿；待真实模型回合验收。
- [ ] implemented_unverified（2026-09-25）：用户对单镜长短的原话偏好经 `shotLengthHint` 从 Phase 1 透传到 Phase 3，区分快切与长镜；`default` 逐字保持改造前行为。编译、142 项 storyboard 测试与 harness 通过，待真实模型回合验收。见 docs/changes/2026-09-25-shot-length-hint.md。
- [ ] implemented_unverified（2026-09-25）：Agent 最后一个模型步骤改为只总结已确认产物与未完成事项，避免预览已生成后继续多轮读工具而落入步骤上限。单元回归和 Release 编译通过；重建版桌面读取既有演示项目成功，最后一步路径待真实模型回合触发。
- [ ] 实施中（2026-09-25，claude/product-naming-voycut-3663a1）：对外品牌由 FellowCut 改为 Voycut（按设计稿字标写法），保留应用标识、数据库、凭据服务与语言偏好存储键以兼容本机旧数据；图标待设计源文件到位后更换；构建并验收更名安装包。网站在独立仓库实施。见 docs/changes/2026-09-25-rename-to-voycut.md。
- [ ] implemented_unverified（2026-09-24）：修复发送时界面短暂卡住。任务归属模型请求与对话提交在后台工作线程执行，发送时显示任务归属状态；待真实桌面交互验收。见 docs/changes/2026-09-24-send-ui-responsiveness.md。
- [ ] 进行中（2026-09-17，feature/agent-led-storyboard）：细拍、一拍一镜、模型主导生成。PR1–PR6 已实现；P3 可见描述+现拼网格已复测（10 拍全附图；5 合格 / 2 勉强 / 3 偏题）。下一步：池内更贴 requiredVisual 的条仍可能落选。见 docs/changes/2026-09-17-p3-visible-select.md。
- [ ] 已实现，待审查/合并（2026-09-09，codex/light-workspace-shot-replacement）：B 浅色双栏、Top-12 持久化、手动替换及撤销/重做；桌面烟雾与隔离交互通过。基线已有 3 项校验失败、独立审查额度不足，见 docs/changes/2026-09-09-light-workspace-shot-replacement.md。
- [ ] 后续：合入后用 1–2 个真实项目抽检竖屏 `cropFocus`（预览裁剪与 Jianying 草稿主体是否一致）。
<!-- ACTIVE_TASKS_END -->

## 说明

这里记录的是近期工作状态，不要求每个新任务都同步到长期文档。遇到问题时优先记录复现方式、实际错误和影响，再决定是修代码、补测试还是暂时接受限制。历史实现可以通过 Git 提交记录查看。
