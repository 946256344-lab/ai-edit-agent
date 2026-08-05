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
- 已人工验证的 Jianying Pro 8.0 仅视频草稿创建、首页注册及带视频片段打开。

## 下一阶段

### 1. MVP 验收与可靠性

- 使用真实素材和实验性 Provider 手工验证完整工作流。
- 验证 OAuth 令牌刷新、重启持久性和模型访问。
- 补齐通用工具调用状态、操作日志查询和可恢复的 Agent 运行时。

### 2. 生产化媒体与模型接入

- 捆绑或可靠供应媒体运行时和 Python/Jianying 适配器依赖。
- 实现自定义模型 API 适配器，继续保持 Provider 可替换。
- 实现收集项目媒体、缺失文件恢复策略、质量评分和语义重复检测。

### 3. 创作能力扩展

- 支持多轨音频、字幕、变换和自动化。
- 扩展 Jianying 映射到图片、文本、音乐和 logo。
- 在获得明确契约后接入 voice API。

## 后续方向

- 商业音乐 Provider。
- 更多输出比例、预设和本地化。
- 本地模型 Provider。
- 高级特效、AI 生成素材和数字人能力。
