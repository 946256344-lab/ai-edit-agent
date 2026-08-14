# 2026-08-14 桌面产品事实基线

## 结论

恢复后的应用能够启动，并从本地 SQLite 恢复真实项目、素材和既有产物。第一个破坏核心使用路径的断点不在媒体后端，而在前端工作区结构：本应互斥的 Agent 对话、storyboard、Workflow 与素材管理被同时放进同一长页面或狭窄侧栏，导致标签切换不再等于工作模式切换。

## 真实桌面观察

在 `codex/recovery-baseline-20260814` 分支启动 `npm run tauri:dev`，恢复到一个既有本地项目：

- 素材总数 891；技术分析完成 512、失败 379、分析中与排队均为 0。
- 源文件健康摘要显示 891 正常，缺失、变化、不可读和未检查均为 0。这里的“379 失败”是分析失败，但首屏没有清楚区分失败类别。
- 已恢复 8 镜头 storyboard、8 片段 timeline；界面声明 preview 已生成。
- 向下导航后可以看到本地 preview 的真实媒体画面，说明既有产物和本地媒体 URL 至少能够加载；本轮没有据此声称完整播放、重新渲染或 Jianying 交付已经重新验收。

## 阻断问题

### P0：模式切换没有隔离内容

复现：在首屏点击“故事板”。

实际结果：标签变为选中，但 Agent 会话标题、完整消息流、项目上下文、审计、preview 与 composer 继续排在 storyboard 前面。该旧会话需要连续翻页才能看到 storyboard。

代码根因：`ConversationWorkspace` 无条件渲染 `.message-stream` 和 `.composer`，随后才用 `activeView` 追加 `.storyboard-view`。初始基线 `9fbaf5d` 使用 `activeView === 'chat' ? ... : ...`，原本具备互斥渲染。

### P0：storyboard 基础样式在扩展过程中丢失

实际结果：导航到 storyboard 后，镜头内容表现为接近浏览器默认排版，按钮、网格和镜头卡没有形成可审阅界面。

代码根因：当前 `App.css` 只在响应式规则中引用 `.storyboard-view` 与 `.shot-grid`，初始基线中的 `.storyboard-view`、`.storyboard-heading`、`.shot-grid`、`.shot-card`、`.shot-image` 和 `.shot-copy` 基础规则已丢失。

### P0：素材完整工作台被放入固定窄侧栏

实际结果：`AssetManagementPanel` 的三栏网格被放进应用第三列约 330px 的区域，素材状态、筛选、目录、列表与 Inspector 同时争抢空间并发生裁切。

代码根因：组件 CSS 明确按三栏完整工作台设计，但 `App.tsx` 把它作为 `.workspace` 的并列第三列渲染；现有 `.workspace > .asset-workbench` 规则反而表明实现曾准备把它放回主工作区，却没有完成组合层迁移。

### P1：Workflow 重复且动作不随产物收敛

`App.tsx` 与 `ConversationWorkspace` 各渲染一套 Workflow。已有 timeline 和 preview 时，“创建时间线”仍保持首要动作，状态事实、下一步和历史操作混在同一区域。

### P1：失败状态语义不清

首屏同时出现“379 失败”和“891 正常”。数据可以同时成立：前者是分析状态，后者是源文件健康；但界面只写“失败”，用户无法判断是否需要重新定位、重试分析或忽略。

## 变更历史可追溯性

- 初始提交 `9fbaf5d` 的 Agent/storyboard 条件渲染和 storyboard 样式是完整的。
- 后续只有缓存忽略提交 `815ce92`，直到恢复快照 `8020d73` 才一次性提交 95 个文件、约 2.3 万行新增内容。
- 因为中间实现没有形成可运行的小提交，无法从 Git 精确定位哪一次修改首先破坏布局。文档表明 `ConversationWorkspace` 拆分发生在后期 Agent 完成态重构附近，但这只能作为推断，不能冒充可验证提交历史。

因此，Markdown 约束不是完全没有作用；它记录了后端契约和目标。但它没有约束“工作模式必须互斥”“每次结构重构必须进行桌面截图验收”“高风险变更必须形成可回退的小提交”。Harness 只能验证要求的文档被修改，不能验证文档描述与真实界面一致。

## 下一修复切片

1. 将顶层模式收敛为 Agent、素材、成果，任一时刻只渲染一个主工作区。
2. 把 `AssetManagementPanel` 放入完整宽度主工作区。
3. 成果页集中展示唯一 Workflow、storyboard、timeline/审计和 preview。
4. 恢复 storyboard 基础样式，固定应用高度，让各模式使用自己的内部滚动容器。
5. 通过真实桌面截图验证模式切换，再继续 Provider 与新请求闭环验收。
