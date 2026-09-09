# 2026-09-09：剪映交付可见反馈

## 触发范围

- `src/hooks/useArtifactWorkspaceController.ts`
- `src/components/ArtifactsWorkspace.tsx`
- `src/App.css`
- `src-tauri/src/jianying.rs`

## 行为变化

- 成果页按钮改名为「生成剪映草稿」（不再暗示会自动打开剪映）。
- 交付失败时在成果页显示可读中文原因，不再静默吞错。
- 交付成功时显示草稿名称与「退出/重启剪映后可见」说明。
- 新建草稿名改为「项目名-短后缀」，便于在剪映草稿箱中查找。

## 验证

- `npm run lint`
- `npm run build`
- `cargo test --manifest-path src-tauri/Cargo.toml jianying::`
