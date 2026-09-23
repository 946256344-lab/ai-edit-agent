# 正式版模型端到端验收

## 范围

在完整 Release 程序中使用已配置的自定义模型 API，验证首次项目从素材导入到可播放预览的真实闭环。测试数据均为临时生成，不依赖开发服务器、系统 FFmpeg/Python/Tesseract 或已有项目。

## 结果

- 540×960 临时 H.264 素材完成本地探测、缩略图、切段与 OCR，随后完成模型视觉分析并写回 5 个视觉标签。
- 普通无工具对话完成，证明模型在一次短暂 HTTP 429 后能正常恢复。
- 合法的 3 秒请求以 `completed` 终态结束；`list_assets`、`generate_storyboard`、`render_preview`、`get_edit_status`、`get_storyboard`、`get_timeline` 六步全部成功。
- Storyboard 为 1 beat、1 镜头、0 未覆盖；时间线为 1 个视频片段、0 质量警告，未写入配音、字幕或 BGM。
- 随包 FFprobe 确认预览为 H.264、540×960、2.966667 秒，可读取播放。
- 2 秒非法请求被明确拒绝并建议调整到 3 秒，未伪造成功结果。
- 每轮临时项目、数据库记录与临时源文件均在验证后清理；正式数据库恢复为 0 项目、0 素材、0 Agent 任务、0 storyboard、0 timeline。

## 当前仍需人工决定

- NSIS 安装包尚未进行 Authenticode 数字签名。
- Git 仓库尚无版本标签或 GitHub Release，当前 GitHub CLI 也未登录。
- 本机未发现剪映或 CapCut 草稿目录，编辑器交付需要在已安装并至少启动过一次的机器上验收。
- 正式发布前仍应选一组代表性真实素材做内容质量验收；本轮证明功能闭环，不代表真实选镜质量。
