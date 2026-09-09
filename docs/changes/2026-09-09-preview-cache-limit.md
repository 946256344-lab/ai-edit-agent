# 2026-09-09：预览缓存上限与清理

## 触发范围

- `src-tauri/src/preview_cache.rs`
- `src-tauri/src/preview.rs`
- `src-tauri/src/projects.rs`（启动孤儿缓存清扫）
- `src-tauri/src/lib.rs`
- `src/lib/local-store.ts`
- `src/components/ProjectSettingsModal.tsx`
- `src/components/AppSidebar.tsx`
- `src/App.tsx`
- `docs/api.md`
- `TASKS.md`

## 改动

- 单项目 `previews/cache/<projectId>` 默认上限 2 GiB；`render_preview` 成功后按修改时间淘汰最旧中间文件。
- 新增 `get_preview_cache_status` / `clear_preview_cache`（须确认）。
- `initialize_local_store` 删除已不在 `projects` 表中的孤儿缓存目录（当前无整项目删除命令，此为残留清理）。
- 侧栏「项目设置」打开维护弹窗，可查看占用并清理当前项目缓存。

不删除 timeline 最终 preview 目录、素材或 SQLite 数据。

## 同步文档

- `docs/api.md`
- `TASKS.md`
- `docs/changes/2026-09-09-preview-cache-limit.md`

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml preview_cache::`
- `npm run lint`
- `npm run build`
- `npm run harness:check`（若适用）

## 决策

无 ADR。上限取 2 GiB；整项目删除命令仍未实现，孤儿清扫覆盖“项目记录已不存在”的情况。
