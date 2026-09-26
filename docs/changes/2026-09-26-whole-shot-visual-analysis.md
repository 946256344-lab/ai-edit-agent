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

## 追加：角标被当成画面文字（同日）

重新识别后抽查：18.mp4 的 `onScreenText` 是「1 0.8s」「2 2.3s」……，`textLanguages` 因此成了 English。提示词写明格子角标是我们加的，不是画面文字；Rust 再滤掉「编号 秒数」形式的条目，滤空后语言一并清空。

同次抽查另见：主体横向位置不可靠（18.mp4 人在画面右侧却 6 帧都标 center）；25.mp4 整画面虚化仍判 out_of_focus。暂不处理。

## 追加：一大四小（同日）

用户判断 6 帧网格信息不够，定为每段一张图「1 帧大图 + 4 帧小图」：
- 抽样改为每段 5 帧，按 960 宽抽，清晰度评分先缩回 320 宽再算，标准不变；
- `compose_feature_frame_sheet` 把中间帧放大为 960×540 放在左边，其余 4 帧 320×180 叠在右边（画布 1280×720）；竖拍改为 405×720 大图加 2×2 小图；各帧按原比例放进格子；
- 提示词说明大图看细节、小图看变化，编号按时间顺序，主体横向位置只问大图。

原 3×2 网格函数删除。浅景深/失焦的区分按用户意见暂不处理。

## 追加：按时长加图，补充信息（同日）

用户定帧数规则：每 15 秒一张「一大四小」图，15 秒内 5 帧、15–30 秒 10 帧、30–45 秒 15 帧，以此类推。单请求最多 4 张图，超过 60 秒的段固定 4 张 20 帧并在整段均匀铺开（库里仅 3 段超过 60 秒）。编号在整段内连续，一段的全部图放在同一个请求。

补充要模型给出（水印按用户意见暂不做）：
- `highlights`：高光时刻；
- `subjectSpans`：每张大图里主体左右边界（画面宽度比例），五档位置由它换算；
- `verticalCropFit`：竖屏 9:16 能否保住主体；
- `cleanStart` / `cleanEnd` / `edgeNote`：开头结尾能否直接做剪辑点；
- `subjectDirection` / `cameraDirection`：运动方向；
- `setting` / `timeOfDay` / `colorTone` / `brightness`：光线色调；
- `peopleCount` / `facesVisible` / `safetyGear`：人物与防护装备；
- `concepts`：可表达的抽象概念；`mood`：氛围。

变化、高光、概念与氛围进入召回语义与词面文本；全部进入 Phase 3 候选卡，提示词要求结合相邻拍的连贯性使用。请求超时 60→90 秒。按用户意见先不拆成两个请求，信息多到模型答不好时再拆。

## 同步文档

`docs/api.md`、`docs/architecture.md`、`docs/decisions.md`、`TASKS.md`。

## 验证

`cargo test --lib` 437 条通过（抽样改为 6 帧的测试、整段字段宽松解析与夹回段内的测试、请求内容测试）；实拼 91.mp4 网格，标注清晰。待桌面重新识别后抽查描述与新字段。
