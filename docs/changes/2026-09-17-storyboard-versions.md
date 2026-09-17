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
