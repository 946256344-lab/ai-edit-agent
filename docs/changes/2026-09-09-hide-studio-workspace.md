# 2026-09-09：发行 UI 隐藏 Studio 工作台

## 触发范围

- `src/components/workspace-types.ts`
- `src/components/WorkspaceHeader.tsx`
- `src/App.tsx`
- `TASKS.md`

## 改动

发行界面不再暴露第四个顶层模式「工作台」。`WorkspaceView` 收回到 `chat | assets | artifacts`；顶栏去掉入口，`App.tsx` 不再挂载 `StudioWorkspace`，也不再初始化 `useStudioWorkspaceController`。

Studio 相关源码、MovieMasher adapter、`commit_studio_edits` 与 timeline persistence 全部保留，不改 Agent 剪辑、storyboard、preview 或 Jianying draft。

## 同步文档

- `TASKS.md`
- `docs/changes/2026-09-09-hide-studio-workspace.md`

`docs/architecture.md` / `docs/roadmap.md` 原先已写「Agent、素材、成果」三个互斥顶层模式，与本次 UI 对齐，未改正文。

## 验证

- `npm run lint`
- `npm run build`

## 决策

无 ADR。Studio 能力仍作为内部实现保留，只对用户隐藏入口。
