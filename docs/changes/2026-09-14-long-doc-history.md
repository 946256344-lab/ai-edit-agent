# 长期文档历史补充

原实现与验证记录按当时文档保留，不代表当前契约。

## 原长期文档补充：素材详情预览补充（2026-09-14）

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

`get_asset_evidence(assetId)` 的 `AssetEvidence` 增加 `kind`（video/image/audio/other）和 `mediaPath`（本地源媒体路径）。命令从当前素材记录读取路径，仅将该文件加入本次应用进程的 asset protocol scope；全局目录 scope 不变，不复制、转码或修改源文件。前端路径只用于 `convertFileSrc`，不显示为用户文案。

素材详情以原片预览、源场景片段、视觉分析、折叠 OCR 的顺序展示。有场景分段时仍展示素材级 `visualEvidence`，不将素材级描述冒充某个片段的分析。视频片段使用 `startMs/endMs` 定位和停止播放；若有运动可用窗则播放 `usableStartMs/usableEndMs`。每个片段可展示 `motionEnergy[]` 曲线，色带标出可用窗；点击曲线可定位预览。原格式是否可播放取决于 WebView 的媒体解码支持，失败显示不可播放状态。

维护记录（2026-09-15）：素材切段只认 FFmpeg 硬切并用 CLIP 验真；无已验证硬切则整条一段。`analysisVersion=3`。见 `docs/changes/2026-09-15-hard-cut-segments.md`。
维护记录（2026-09-15）：硬切片段内用帧差运动能量收缩可用窗。`analysisVersion=4`。见 `docs/changes/2026-09-15-motion-energy-trim.md`。
维护记录（2026-09-16）：素材详情展示片段运动能量曲线与可用窗。见 `docs/changes/2026-09-16-motion-energy-detail.md`。

维护记录（2026-09-26）：片段视觉证据带 `detail`（导入时整段识别）时，素材详情在该片段下展示「识别细节」：变化、高光、最佳区间、主体位置、竖屏裁切、开头结尾、运动方向、焦点、场景光线、人物、画面文字、品牌标识、人群/展会、抽象概念、氛围；枚举值按界面语言显示，模型自由文本保持原文，空项不显示。前端类型为 `local-store.ts` 的 `ShotDetail`，命令返回值不变。见 `docs/changes/2026-09-26-shot-detail-in-asset-panel.md`。
维护记录（2026-09-18）：技术分析先扫关键帧，缩略图/抽帧超时不整条失败。见 `docs/changes/2026-09-18-faster-asset-analysis.md`。
