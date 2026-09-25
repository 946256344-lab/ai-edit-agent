# 2026-09-25：选镜局部编辑工具（reselect_shots / refine_shot_ranges）

## 结果

生成后用户只对个别镜头不满意时，Agent 不再重跑整条 P1–P5。新增两个按编辑意图命名的工具，在 Rust 里走固定子流程，只改指定 beat / 镜头，其余冻结：

- `reselect_shots`：指定 beat 的 P2 召回 → P3 看图选镜 → P4 精修 → P5 校验。
- `refine_shot_ranges`：只跑 P4 → P5，素材与 segment 锁死，只调切点与 cropFocus。

`generate_storyboard` 保留为首次剪辑与改目标的默认流程；用户点名素材仍走 `replace_clips`。

## 设计决定

1. **拍时长以当前时间线为准。** 生成后配音对齐已落进 clip 时长；局部重选时每拍时长 = 该拍 clip 时长之和，配音不动、时长不动。与 `shot_replacement` 现有做法一致，不另存 SpeechTiming。
2. **每次局部修改写一对新版本。** 派生 storyboard 版本（`content_json` 新增 `derivedFromVersionId`、`changedBeatIds`，serde 默认值兼容旧数据，不改 schema）+ 候选池；新 timeline 从当前 timeline 复制，只换相关 clip，保留配音、字幕、音乐与叠加轨。
3. **派生基准是当前时间线。** 用户此前用 `replace_clips` 改过的其他 beat，派生时以时间线 clip 为准写回 shot，并按源范围找回所在 segment；找不到则 `segmentId` 置空。
4. **默认排除当前素材。** 用户说「不好」即不要它；`keepCurrent=true` 才保留在池中。
5. **P5 不是可选工具。** 每个写工具末尾都做 normalize + 全片校验（冻结镜头不动），不过就不写入。
6. **P2 带完整信号。** 语义向量、CLIP、使用次数都参与；用户原话 `instruction` 拼入该 beat 查询文本并进入 P3 / P4 提示词。
7. **诚实失败。** 候选池耗尽或校验不过时返回真实原因，不静默退回整条重跑。

## 工具契约

`reselect_shots`
- 入参：`shotIndexes` 或 `beatIds`（二选一，最多 5 拍）、`instruction`（可选）、`keepCurrent`（默认 false）
- 返回：`storyboardVersionId`、`timelineVersionId`、每拍 `{beatId, before:{assetId,segmentId}, after:{assetId,segmentId}, matchLevel, remainingAlternates}`、`qualityWarnings`

`refine_shot_ranges`
- 入参：`shotIndexes`（最多 10 个）、`instruction`（可选）
- 返回：`storyboardVersionId`、`timelineVersionId`、每镜新旧源范围、`qualityWarnings`

## 护栏

- 同一 beat 一轮最多重选 1 次；超过 5 拍建议改用 `generate_storyboard`。
- 未知 beat / 镜头序号直接报错；无 storyboard 或 timeline 时拒绝。
- 工具描述写清分工：首剪 → `generate_storyboard`；局部不满意 → `reselect_shots`；只调切点 → `refine_shot_ranges`；点名素材 → `replace_clips`。

## 前端

版本列表显示派生关系，例如「v5（改自 v4，第 3 拍）」。`StoryboardVersion` 新增可选字段 `derivedFromVersionId?: string | null`、`changedBeatIds?: string[]`。

## 实施步骤

- M1 纯重构：从 `generate_storyboard_internal` 抽出 P3 选镜循环、P4/P5 精修校验循环、版本落库，行为不变。
- M2 派生版本数据与「时间线 → 拍时长 / 局部替换 clip」工具函数。
- M3 `reselect_shots`；M4 `refine_shot_ranges`；M5 Agent 接线与文档同步；M6 真实桌面验收。
- 另开任务：手动换镜推荐改用完整信号召回；前端派生版本标注。

## 验收场景

1. 「第 3 个镜头换一个」：只该拍变化，配音与其他 clip 源范围逐字不变，预览重出；模型调用只有 1 次 P3 + 1 批 P4。
2. 「第 2 和第 5 个镜头太暗」：两拍都换且互不相似。
3. 「第 4 个镜头晚半秒开始」：只走 `refine_shot_ranges`，素材不变。
4. 「用 xx 素材替换第 1 个」：走 `replace_clips`，不调模型。
5. 池耗尽：诚实报告，不整条重跑。
6. Agent 轨迹：局部抱怨不触发 `generate_storyboard`。
