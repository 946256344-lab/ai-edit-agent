# 当前技术决策

本文只记录今天仍然影响开发的决定；历史 ADR 仍可在仓库历史中查阅，不再作为必读规则。

## 1. 本地优先

项目数据、原始媒体引用、分析结果、storyboard、内部 timeline 和 preview 默认保存在本机。导入不会修改原始媒体。

## 2. Rust 负责本地副作用

React 通过 Tauri 命令表达意图，Rust 负责 SQLite、文件、媒体工具、模型请求和产物创建。前端不直接访问数据库或本地文件。

## 3. Agent 使用工具完成剪辑

自然语言请求进入 Agent 工具循环。模型可以观察项目状态、选择素材、生成 storyboard、编辑 timeline 和请求 preview；模型文字本身不能证明产物存在。

## 4. Storyboard 先于 Timeline

Storyboard 必须基于已分析的真实素材证据，并记录素材 ID 和源时间范围。内部 timeline 从已验证的 storyboard 创建，不能凭文件名或模型自述猜测媒体内容。

## 5. 产物使用新版本

Storyboard、timeline、preview 和 Jianying draft 不覆盖旧版本。Jianying draft 是从内部 timeline 创建的单向交付物，不反向同步。

## 6. Provider 可替换

模型连接通过统一 Provider 入口，支持 OAuth 和自定义 OpenAI 兼容 API。凭据保存在 Windows Credential Manager，不写入 SQLite、浏览器存储或普通日志。

## 7. 失败要可见

Provider、媒体工具或数据库失败时返回真实且可理解的错误。不要用假成功或静默 fallback 掩盖错误。重试只在确实能改善临时网络问题、且不会重复本地副作用时使用。

## 8. 用户确认不可逆操作

最终导出、覆盖已有导出、删除项目或素材等不可逆操作必须先获得明确确认。当前 Jianying draft 创建只生成新的草稿目录。

## 9. Preview 是本地检查产物

Preview 使用本地 FFmpeg 生成，用于检查节奏、字幕和画面。Preview 不是最终导出，也不会修改原始媒体。

## 10. 低成本优先

早期产品优先简单的本地分析与可解释排序。片段级视觉证据采用**按需 + 永久缓存**：本地场景分段整库后台补跑（不花钱），视觉模型只对进入粗召回的素材调用。遇到质量问题，先增加复现和测试，再决定是否扩大模型调用。

## 11. 片段级分析取代固定 4 帧（2026-09-09）

技术分析以 FFmpeg 低分辨率场景检测生成真实片段（`analysisVersion=2`）；选镜候选单位为片段。Phase 2 每 beat 先召回 9 条互不相似整片，有 `scene_segments` 则展开为片段（视觉超时仍锁片段与源范围，不退整条）。Phase 3 返回 `assetId+segmentId`，同一 beat 禁止同片；跨 beat 允许同一素材的不同、不重叠、不相似片段。已用片段的相似画面硬拒。仅当素材没有场景段时才按整条参与。CLIP / 视频 embedding 另议。

## 12. key_message 只出字幕标记不配音（2026-09-08）

`key_message` 是短目标/提纲成片：Phase 1 为每个 beat 写屏幕标记 `onScreenText`（≤24 可见字符），`narration` 留空；镜头时长按标记可读性与目标时长分配（`SpeechTiming.kind=pacing`）。时间线为每个 beat 写一条跨该 beat 镜头的标记字幕，**不自动配音**。显式 `synthesize_voiceover` 仍可用，但不得朗读 `onScreenText`，仅在 beats 仍有 narration 时合成。`full_script` 继续走口播 + audio-first + 自动配音。

## 13. scriptMode 由系统在 Phase 1 前锁定（2026-09-08）

`scriptMode` 是旁白/字幕产品路径的开关，不得交给模型自选。Rust 用 brief 朗读估算（≥约 20s → `full_script`，否则 `key_message`）写入 Phase 1 必选约束；响应后再强制钉死。模型只负责在锁定模式下拆 beat 结构。

## 14. 当前未完成事项

- 安装包还没有完整捆绑 FFmpeg、Tesseract、Python 和 Jianying 适配器运行时。
- 最终视频导出尚未实现。
- 多轨媒体能力仍在迭代。
- 官方模型 OAuth 契约和部分外部 Provider 能力仍需真实环境验证。
- 浅色工作区替换面板尚未适配片段候选卡（`shot_replacement.rs`）。
