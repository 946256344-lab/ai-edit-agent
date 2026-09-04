# 2026-09-04：剪辑会话可右键删除

## 变更

侧栏「剪辑会话」支持右键菜单删除。删除对象为 editing task（UI 剪辑会话），在显式确认后级联清除该任务下全部 conversation/消息、Agent 记录、storyboard/timeline，并删除对应本地 preview 目录；项目级素材与外部 Jianying 草稿保留。

## 文件

- `src-tauri/src/projects.rs`：`delete_editing_session` / 级联清理
- `src-tauri/src/lib.rs`：注册命令
- `src-tauri/src/agentloop/native.rs`：任务行缺失视为已取消
- `src/lib/local-store.ts`、`src/components/AppSidebar.tsx`、`src/App.tsx`、`src/App.css`
- `docs/api.md`、`docs/architecture.md`

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml`
- `npm run build`（若前端类型有变）
