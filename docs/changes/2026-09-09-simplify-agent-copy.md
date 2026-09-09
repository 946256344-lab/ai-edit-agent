# 2026-09-09：简化 Agent 文案与运行阶段

## 触发范围

- `src/components/AgentWorkspace.tsx`
- `src/components/AgentRunCard.tsx`
- `src/App.tsx`（对话欢迎语与失败提示）
- `src/components/WorkspaceHeader.tsx`（本地项目文案）
- `src/hooks/useArtifactWorkspaceController.ts`（写入对话的交付提示）
- `TASKS.md`

## 改动

用户可见文案去掉 storyboard / timeline / tool / Jianying draft / local preview 等内部术语。

- Agent 引导改为「告诉我你想剪什么，我会分析素材并生成第一版视频。」
- `AgentRunCard` 折叠摘要按阶段展示：分析素材 → 选择镜头 → 生成剪辑 → 生成预览 → 准备剪映草稿 / 完成
- 展开步骤仍显示更具体但仍面向用户的动作名；产物改为「镜头方案 / 剪辑结果 / 预览 / 剪映草稿」
- 保留停止按钮与取消/失败不覆盖已有结果的说明

同分支还包含隐藏 Studio 与成果页默认简化（见同日 change records）。

## 同步文档

- `TASKS.md`
- `docs/changes/2026-09-09-simplify-agent-copy.md`

## 验证

- `npm run lint`
- `npm run build`

## 决策

无 ADR。侧栏「剪辑会话」与 Agent 确认门未改，留给后续切片。
