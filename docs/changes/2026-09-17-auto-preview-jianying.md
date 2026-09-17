# 生成后自动预览并新建剪映草稿

分支：`feature/agent-led-storyboard`（PR5）。

## 结果

`generate_storyboard` 在时间线有可播镜头时自动渲染预览，并新建一份剪映草稿。收尾缺口只进 `qualityWarnings`，不再推迟预览。已有配音的故事版禁止改旁白，后续只改画面。

## 范围

- `src-tauri/src/agentloop/skills.rs`：能播就预览+新建剪映
- `src-tauri/src/agentloop/tools.rs`、`native.rs`、`continuation.rs`：禁止改旧旁白
- `src/hooks/useArtifactWorkspaceController.ts`：手动生成路径同样自动建剪映草稿

## 禁止变化

- 不覆盖已有剪映草稿
- 不改公开 Tauri 命令名
- 预览或剪映失败不回滚已保存的故事版
