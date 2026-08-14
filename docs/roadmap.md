# 路线图

## 产品目标

交付一个 Windows 桌面 AI 视频剪辑 Agent。用户导入本地视频、图片、文案、音乐和 logo，通过自然语言协作，让 Agent 基于真实媒体分析生成 storyboard、可编辑内部时间线、preview，并创建新的 Jianying Pro 草稿。

Jianying 草稿创建不等同于最终视频导出。最终视频导出仍未实现，且未来必须要求明确确认。

## 当前阶段：本地桌面基础已具备，进入端到端验收

MVP 尚未验收。验收证据必须是在 Tauri 桌面应用中，由已认证的模型 Provider 使用真实导入媒体生成源时间绑定 storyboard，并成功生成可播放的本地 preview。浏览器 UI、未认证的 FFmpeg 测试或模拟数据均不构成验收证据。

已具备：

- Tauri 2、Windows MSI/NSIS 打包、SQLite 迁移和本地项目/剪辑任务/会话持久化。
- 原生媒体导入、源引用可用性检测、FFprobe、FFmpeg 缩略图/关键帧/场景候选和英文 OCR。
- 实验性 OAuth PKCE、Windows Credential Manager、最小帧视觉证据和证据校验 storyboard。
- 内部时间线、局部片段替换、540 x 960 preview、质量检查和受限自然语言工具控制。
- 项目内 Task Resolver：先选择/原子创建剪辑任务，再凭一次性 route receipt 进入 Conversation Router 与 Agent loop；不确定归属先澄清。
- 对话内可折叠 Agent 执行卡：显示安全步骤状态、运行时长和后端确认的真实产物，不展示模型内部推理。
- Agent、素材、成果三个互斥顶层模式；素材工作台使用完整主区域，成果页集中显示 storyboard、timeline/审计和 preview。
- 自定义 Provider 的真实桌面只读链路已验收：精确状态查询不创建 Agent task，项目事实问答通过观察工具完成，task 终态、确定性回复和 conversation 状态原子持久化；模式切换和 WebView 刷新后可恢复。
- 自定义 Provider 的真实桌面写链路已验收：同一 task/storyboard 新建内部 timeline v5 和对应 540 x 960 local preview，旧 v4 timeline/preview 保留；preview 可实际播放，WebView 刷新和 Tauri 重启后恢复。`submit_conversation_turn.run.agentTaskId` 契约和终态轮询竞态已修复并通过无刷新完成对账。
- 已人工验证的 Jianying Pro 8.0 仅视频草稿创建、首页注册及带视频片段打开。

## 下一阶段

### 1. MVP 验收与可靠性

- 使用真实素材和实验性 Provider 手工验证完整工作流。
- 验证 OAuth 令牌刷新、重启持久性和模型访问。
- 继续验证 Agent 完成事件主动丢失和修复后快速完成场景；长运行任务的真实 `agentTaskId` 对账、窗口切换、WebView 刷新、Tauri 重启、终态回复幂等持久化及历史缺失回复恢复已在真实桌面通过。
- 已补齐步骤级工具调用状态、作用域查询、统一状态快照、确定性前置条件提示和回归评测资产；下一步继续实现持久化队列、暂停/恢复与人工审阅后的明确续跑入口。
- 使用真实桌面 Provider 验证从临时任务切回既有剪辑任务、低置信度路由澄清和新任务自动创建。

### 2. 生产化媒体与模型接入

- 捆绑或可靠供应媒体运行时和 Python/Jianying 适配器依赖。
- 实现自定义模型 API 适配器，继续保持 Provider 可替换。
- 实现收集项目媒体、缺失文件恢复策略、质量评分和语义重复检测。

### 3. 创作能力扩展

- 已实现：storyboard 以文案信息点驱动选镜，拒绝把缺少证据的 `insufficient` 画面写入成片；模型以十步编排和三次 storyboard 内存修订决定下一步，并可请求分析项目内未分析素材。下一步是以真实桌面素材验证直接/语境匹配、分析排队与部分完成总结的稳定性。
- 支持多轨音频、字幕、变换和自动化。
- 扩展 Jianying 映射到图片、文本、音乐和 logo。
- 在获得明确契约后接入 voice API。

## 后续方向

- 商业音乐 Provider。
- 更多输出比例、预设和本地化。
- 本地模型 Provider。
- 高级特效、AI 生成素材和数字人能力。
