# 导入时每段 6 帧整段识别

## 现象

导入识别每个硬切段只看中点 1 帧：
- 描述只代表一瞬间，看不到人进出画面、机位移动、焦点变化；
- 分不清浅景深（故意虚化）与失焦；
- 没有画面文字、品牌标识、人群、展会等标记；
- Phase 4 只能再抽帧问一遍动作起止和主体位置（「测试1」中 18.mp4 主体在右侧却按中心裁切）。

## 根因

粗识别单元是每段 1 帧，请求与结构里没有时间维度字段。

## 触发范围

- `src-tauri/src/assets/segments.rs`：每段抽样帧改为固定 6 帧（均分 6 份取中点），清晰度评分与识别共用。
- `src-tauri/src/storyboard/multimodal.rs`：新增 `compose_labeled_frame_grid`，拼 3×2 网格，每格左上角用内置点阵标「编号 时间」。
- `src-tauri/src/assets/visual.rs`：识别单元带整段帧与段范围，每段一个请求（一张网格），各段并发；新提示词要求返回共同描述、景别、运镜、`changes`、`subjectPositions`、`focus`、`qualityNotes`、`onScreenText`、`textLanguages`、`brandLogos`、`crowd`、`exhibition`、`bestRange`；时间按秒宽松解析（数字、`"12.4"`、`"12.4s"`）并夹回段内，枚举字段归一，写不合规的条目丢弃。超时 30→60 秒。按素材分组的 `frames_by_asset` 在每段一个请求下不再需要，已删除。
- `src-tauri/src/models.rs`：`VisualEvidence.detail: Option<ShotDetail>`（及 `ShotChange`、`SubjectPosition`、`BestRange`），旧数据为空。
- `src-tauri/src/storyboard/semantic.rs`、`scoring.rs`：变化描述进入召回语义与词面文本。
- `src-tauri/src/storyboard/phases.rs`：Phase 3 候选卡加 `changes`、`focus`、`bestRangeMs`、`contentFlags`，提示词说明其含义且同样未经核实。

## 改动

旧素材需重新识别（技术分析重抽 6 帧，再整段识别）才有新字段；未重识别的沿用旧描述。Phase 4 暂不使用新字段，下一步再接。

## 同步文档

`docs/api.md`、`docs/architecture.md`、`docs/decisions.md`、`TASKS.md`。

## 验证

`cargo test --lib` 437 条通过（抽样改为 6 帧的测试、整段字段宽松解析与夹回段内的测试、请求内容测试）；实拼 91.mp4 网格，标注清晰。待桌面重新识别后抽查描述与新字段。
