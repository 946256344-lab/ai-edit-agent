# 品牌卡、镜头转场与品牌套件

## 背景

09-27 做 30 秒宣传片时，片头标题、段落字卡和片尾卡都是手写 HTML/CSS 经无头 Edge 截成透明 PNG、再用 ffmpeg 叠加，段间 0.3 秒叠化也是手工加的。产品里没有这些能力：时间线只有文字轨，FCPXML / OTIO 完全不写文字，也没有转场。

按「创作者会不会改」拆开（09-27 与用户确认）：

- 会改的字（标题、分节字、字幕、下三分之一）继续走原生文字，交付成各编辑器可编辑的文字。
- 不会改的品牌元素（logo、渐变字、圆角卡片、片尾卡）用 HTML/CSS 模板渲染成透明 PNG，交付时明确标注「图片，文字不可编辑」，不假装可编辑。
- 模型只填短文案、选模板 id；时间、布局、字体、颜色、动画和渲染都由代码和模板决定，Rust 校验并截断模型输出。

## 改动

### 时间线

`TimelineContent` 平铺新增 `graphicOverlays` 与 `transitions`，旧版本读出为空。

- **品牌卡**
  - 字段：`id`、`templateId`、`anchor`（`opening | closing | whole | at_shot`）、`anchorShotIndex`、`slots { headline, subline, cta }`、`brand`（品牌快照）、`startMs/endMs`、`fadeInMs/fadeOutMs`。
  - PNG 是派生产物，按「模板指纹、文案、品牌快照、画幅」取缓存键，放在 `previews/cache/<projectId>/cards/`，缺了就重新渲染。
- **转场**
  - `default: { kind, durationMs } | null`，加上逐刀 `cuts[{ afterShotIndex, kind, durationMs }]`。`kind` 为 `none | crossfade | dip_to_black`，时长 200–1000 ms。
  - 转场以切点为中心，不改总时长，配音与字幕时间不动。
  - 解析时夹到相邻较短镜头的 40%，短于 100 ms 按硬切处理。
- **每个新时间线版本都重新贴合**（`timeline_graphics::fit_graphics`）：
  - 开场卡：0 起，默认 2.8 秒，最多占全片 40%。
  - 片尾卡：贴片尾，默认 3 秒，不淡出。
  - 角标：填开场卡结束到片尾卡开始之间。
  - 信息卡：跟随锚定镜头，最多 3 张且互不重叠。锚定镜头被删后移除。
  - 逐刀转场的左侧镜头不在了就移除。
  - 覆盖所有写新版本的入口：Agent 改轨、局部重选 / 精修、工作台保存。

### 模板与渲染

- **模板位置**：`src-tauri/templates/cards/<id>/template.html + manifest.json`，外加共享的 `_base.css`、`_runtime.js`，编译期嵌入二进制。
- **首批四个模板**：`opening_title`、`end_card`、`corner_logo`、`info_card`。manifest 声明锚点、默认时长、是否需要 logo 或品牌，以及每个文案槽位的上限（词数、中文字数、字符数）。
- **页面 CSP** 只允许 `https://brand.voycut.local` 的图片与字体，不联网。文案经 JSON 注入，运行时只用 `textContent` 写入，不拼 HTML。标题按 `max-height` 自动缩字，最小到 50%。
- **渲染器**（`cards/renderer.rs`）：
  - 独立线程里用一个隐藏窗口承载 WebView2，数据目录为 `app_local_data/card-renderer`，不占 Tauri 主线程，空闲一分钟自动释放。
  - 两个虚拟主机分别映射卡片页面目录和项目品牌目录。
  - 截图走 DevTools 协议，固定视口和缩放系数 1，背景透明。
  - 输出尺寸必须等于请求尺寸（预览画布 ×2，如 1080×1920），否则判失败。
  - 每一步 20 秒超时，调用方总超时 45 秒。
- **实测**（本机、不启动应用的临时测试）：四张卡首张约 0.6 秒，之后每张 0.15–0.25 秒；隐藏窗口可以截图，透明通道正确。

### 品牌套件

- 存在 `projects.settings_json.brandKit`，字段有名称、网址或账号、片尾 CTA、主色 / 强调色、logo、可选字体；默认转场存在 `settings_json.defaultTransition`。
- 新命令 `get_brand_kit` / `set_brand_kit`。
- logo（PNG / JPG / WebP / SVG，5 MB 以内）和字体（TTF / OTF / WOFF / WOFF2，20 MB 以内）按内容哈希复制到 `app_data/brand/<projectId>/`，从不覆盖或删除，旧时间线的品牌快照因此仍能重建。
- 项目设置里新增「品牌套件」和「默认转场」表单。

### 生成时自动收尾

`generate_storyboard` 在配音、配乐之后：

- 项目设了品牌名称或 logo 时，自动加开场卡（大标题取故事版标题，经截断）、片尾卡（名称、账号、CTA 来自品牌套件）和角标（需要 logo）。
- 项目默认转场不是硬切时写入默认转场。
- 没有可加的内容就不新建版本。
- `appliedMedia` 新增 `brandCards`、`transitions`，只报实际落地的；失败写进 `brandCardsNotApplied`。
- 默认转场为硬切（用户选）。

### 模型工具

两个新工具，严格 schema：

- **`add_title_cards`**：`{ timelineVersionId, cards[{ templateId, shotIndex, headline, subline, cta }], removeTemplateIds }`。
  - Rust 按 manifest 校验模板和槽位，清洗文案（去换行、控制字符、emoji），中文按字数、拉丁文按词数截断。
  - 返回 `copyAdjustments`，并在结果里说明卡片在编辑器里是图片。
  - 开场、片尾、角标每种只保留一张，信息卡按镜头去重。
- **`set_transitions`**：`{ timelineVersionId, kind, durationMs, afterShotIndices }`。`afterShotIndices` 为 null 时改默认值并清掉逐刀设置。
- 两个工具都要求用户本轮原话提到标题 / 卡片 / logo / 片尾，或转场 / 叠化 / 硬切，否则拒绝，理由同 `media_options::guard_model_options`：flash 模型会自行加用户没要求的东西。
- `get_text_capabilities` 返回模板目录、`brandKitSet` 和各编辑器的交付矩阵。

### 预览

- 有转场时，各镜头按转场的一半向两侧取余量渲染。源素材够就用真实画面，不够时在拼接时冻结首帧或尾帧补齐，不改变镜头本身的速度。
- 拼接改用 `xfade`：叠化用 `fade`，黑场过渡用 `fadeblack`，无转场的切点用 `concat` 接。没有转场时仍走原 concat 路径，原缓存不失效。
- 品牌卡以 `-loop 1` 输入，`fade=alpha=1` 淡入淡出，按时间段叠加在最上层（盖住字幕和叠加画面）。
- 卡片渲染失败不挡预览，写入质量报告 `brand_cards`（info 级，不触发自动修正）。
- 分层缓存键升为 `layers-v3`。
- 实测：三段 2 秒测试画面，叠化加黑场过渡，成片 6.03 秒；取样确认切点处混合、黑场由黑转蓝、卡片按时间出现。

### 交付

| 元素 | 剪映 / CapCut | FCPXML（Resolve / FCP） | OTIO（Resolve） |
|---|---|---|---|
| 原生文字 | 可编辑文字，按已验证矩阵（不变） | Basic Title：文字、字体、字号、颜色可编辑，位置与动画不带 | 没有文字轨，写成 V1 marker |
| 品牌卡 | PNG 复制进草稿 `voycut-cards/`，放在上层视频轨，按时间不重叠分轨，带渐显 / 渐隐 | PNG 复制到导出文件旁 `<名>-cards/`，作为连接的图片片段 | PNG 同上，V2 起的图片轨，按不重叠分轨 |
| 叠化 | 原生「叠化」 | Cross Dissolve | SMPTE_Dissolve |
| 黑场过渡 | 原生「闪黑」（近似） | 不交付，保留硬切 | 不交付，保留硬切 |

- 交付结果新增 `notes[{ code, detail }]`，前端按界面语言翻译后附在交付提示后面。code 有：`brand_cards_as_images`、`brand_cards_failed`、`transitions_unverified`、`transition_not_delivered`、`text_basic_titles`、`text_as_markers`。
- 剪映适配器输入新增 `graphicOverlays`、`transitions`，格式版本不变（只加字段）。转场挂在切点左侧片段上，必须在 `add_segment` 前挂。
- 用随包 SDK 在临时目录实际生成了剪映与 CapCut 草稿，`draft_content.json` 里有两段转场、一张 photo 素材、渐显 / 渐隐动画和独立的 `assembly-brand-0` 轨。剪映的「闪黑」是 `is_overlap=false`，CapCut 的是 `true`。

### 顺带修复

原生文字锚点 `top` / `center` 在 ASS 里一直落到底部对齐（`ass_alignment` 只认 `top_center` 这类写法），居中标题在预览里偏下。

## 已知限制与待验收

- 剪映、CapCut 里的转场、品牌卡图片和动画，以及 Resolve 对 FCPXML 标题、图片、叠化和 OTIO 转场、marker 的导入效果，都还没在真实编辑器里打开确认。交付说明因此带 `transitions_unverified`。叠化在编辑器里是否要求源素材余量、剪映会不会因为重叠转场缩短总长，需要桌面验收时确认。
- 原生文字的剪映「已验证」矩阵没有扩展：`storyboard_text_tracks` 直接把默认描边 + 阴影字幕标为 `verified`，而 `validate_text_tracks` 会把同样样式判为 `local_preview_only`，两处不一致。这次没改交付门槛，已登记为待办。
- 品牌卡是整画布 PNG，编辑器里只能整体移动、缩放或删除。
- 与同日 master 的音乐驱动剪辑合并：FCPXML 里配乐、配音、标题、品牌卡统一作为连接片段挂在所在主线片段下；OTIO 旁白、音乐分轨，品牌卡另起图片轨。

## 同步文档

`docs/api.md`、`docs/architecture.md`、`docs/decisions.md`、`docs/codebase/STRUCTURE.md`、`TASKS.md`。

## 验证

- `cargo check` 通过。
- `cargo test --lib` 全量 447 条中 446 条通过。唯一失败的是 `process::hidden_command_drains_both_pipes_before_waiting_for_exit`：PowerShell 在 5 秒内写 1 MB，全量并发时超时，单独运行通过，与本次改动无关。
- 契约测试 `agent_contract_assets` 通过。
- 剪映适配器 Python 测试 23 条通过（新增 2 条：品牌卡复制与分轨、转场映射）。
- `npm run lint`、`tsc -p tsconfig.app.json`、`npm run i18n:check`、`npm run harness:check` 通过。
- 未启动桌面应用；待桌面验收（见上）。
