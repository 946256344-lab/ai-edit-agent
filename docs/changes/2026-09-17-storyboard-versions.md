# 列出并打开故事版版本

分支：`feature/agent-led-storyboard`（PR6）。

## 结果

剪辑任务可以列出全部故事版并打开其中一版。Agent 状态快照和 `get_storyboard` 使用当前打开的那一版。再次生成仍新建版本并自动切过去。

## 范围

- `list_storyboard_versions`、`get_storyboard_version` 只读命令
- `src/lib/local-store.ts`、`docs/api.md`
- 工作台标题栏版本选择
- Native 快照按打开的故事版 ID 读取

## 禁止变化

- 不覆盖旧故事版；生成始终 INSERT 新行
- 不改已有公开命令名

## 原长期文档补充：2026-09-17：列出并打开故事版版本

来源：`docs/api.md`，文档整理前的历史表述；现状以长期文档为准。

新增只读命令 `list_storyboard_versions` 与 `get_storyboard_version`。工作台记住当前打开的故事版；Agent 快照和 `get_storyboard` 使用该版本，不是永远最新一版。`generate_storyboard` 仍始终新建版本并切到新版。见 `docs/changes/2026-09-17-storyboard-versions.md`。

`StoryboardVersion` 加性可选字段 `derivedFromVersionId?: string | null`（局部改镜所依据的故事版 id）与 `changedBeatIds?: string[]`（改动的拍）。旧版本与普通生成版本缺省或为 null；前端版本切换据此显示「v5（改自 v4，第 3 拍）」，来源不在已加载列表时省略来源，拍位置解析不到时只报数量。见 `docs/changes/2026-09-25-derived-storyboard-version-label.md`。
