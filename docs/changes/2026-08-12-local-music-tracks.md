# 本地音乐轨第一阶段

内部时间线新增版本化音乐轨。Agent 仅能选择当前 local project 中分析完成的音频素材，cue 必须有明确源时间范围和 timeline 范围；后端校验循环、音量和淡入淡出，并创建可审计的新版本。

local preview 使用 FFmpeg 在固定 48 kHz 采样率下按 cue 的源范围裁剪、循环、按毫秒延迟、淡入淡出和混音，不修改源媒体；合成音频回归测试覆盖循环音频轨落地和禁用轨跳过。Jianying 音乐映射尚未真实验证，因此含音乐轨的 draft 会安全拒绝。TODO：完成目标 Jianying 版本的映射与试听验收后，才可提升为可交付能力。

后续验证：本机 `pyJianYingDraft` 提供 `AudioMaterial`、`AudioSegment` 与音频轨 API，适配器已据此写入源范围、循环拆段、音量和首尾淡入淡出。使用合成 3 秒视频和 1 秒音频创建、注册了一个唯一的实验性 Jianying draft，并检查到 1 条音频轨、1 个音频素材和 3 个循环片段。该验证尚未在 Jianying UI 中试听，不得声称音乐已具备正式交付兼容性。
