# 2026-09-09：精简素材页默认展示

## 触发范围

- `src/components/AssetManagementPanel.tsx`
- `src/components/asset-workspace/AssetBrowser.tsx`
- `src/components/asset-workspace/AssetSourceRecovery.tsx`
- `src/App.css`
- `TASKS.md`

## 改动

素材页默认收敛为导入 + 目录 + 缩略图 + 用户可读状态：

- 状态摘要改为「已就绪 / 分析中 / 无法读取」
- 卡片不再打开 Evidence Inspector；技术证据面板默认不出现
- 右侧栏仅在源文件缺失/变化/不可读、健康检查进行中，或重链路预览打开时出现
- 恢复文案改为「发现 N 个素材文件已移动或无法读取」+「重新定位素材」
- 无问题时布局改为两栏，不常驻健康检查与证据空态

Controller、健康扫描与重链路后端能力保留，不改 schema。

同分支还包含隐藏 Studio、成果页默认简化与 Agent 文案简化（见同日 change records）。

## 同步文档

- `TASKS.md`
- `docs/changes/2026-09-09-simplify-assets-workspace.md`

## 验证

- `npm run lint`
- `npm run build`

## 决策

无 ADR。Provider 设置精简与侧栏会话隐藏本轮不做。
