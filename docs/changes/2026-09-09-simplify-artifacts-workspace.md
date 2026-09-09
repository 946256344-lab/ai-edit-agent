# 2026-09-09：成果页默认只看 Preview 与交付

## 触发范围

- `src/components/ArtifactsWorkspace.tsx`
- `src/hooks/useArtifactWorkspaceController.ts`（仅 `getDeliveryStatus` 文案）
- `src/App.tsx`（`continueAdjust`）
- `src/App.css`
- `TASKS.md`

## 改动

成果页默认只展示：

1. Preview（或空状态提示）
2. 用户可读状态摘要
3. 「打开剪映」
4. 「继续调整」（回到 Agent）

Storyboard 镜头列表、文本轨、质量检查、`AgentAuditPanel`，以及「创建时间线 / 生成 preview / 手动生成故事板」全部收入「查看详情」，作为兜底保留，不删除命令或改后端 pipeline。

同分支还包含发行 UI 隐藏 Studio「工作台」入口（见 `docs/changes/2026-09-09-hide-studio-workspace.md`）。

## 同步文档

- `TASKS.md`
- `docs/changes/2026-09-09-simplify-artifacts-workspace.md`
- `docs/changes/2026-09-09-hide-studio-workspace.md`

## 验证

- `npm run lint`
- `npm run build`

## 决策

无 ADR。手动推进按钮暂留详情区，待 Agent 端到端出片稳定后再决定是否进一步隐藏。
