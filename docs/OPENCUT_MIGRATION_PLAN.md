# OpenCut 完全搬运计划 — 操作感修复

> 目标：把 `C:/tmp/opencut-classic` 的操作感“完全搬进来”，分批合并到 `master`。
> 对标版本：`opencut-classic master 72e6f5e` (shallow)
> 本仓基线：`assembly-video-agent master c2b87b5` (Tauri 2 + Vite 8 + React 19)

## 诊断：为什么操作感极差

- 仅抄了皮：侧栏/配色/时间轴样式（`App.css` 252px + timeline 样式）。
- 没搬心脏：`MediaTime(i64 120_000 ticks)` → `number ms` 导致逐帧漂移；无 `WASM 帧对齐/吸附/磁吸`；无 `控制器状态机`；无 `GPU 合成器`，预览仅 `video + div`。
- 直拷 `apps/web/src/*` 必败：`@/ / zustand / hugeicons / next/image / opencut-wasm / bun:test` 全套外部依赖，当前 `tsc -b` 直接 400+ 报错。

## 策略：抽内核，重接存储与交付

- 搬 `time/wasm/media-time + fps + animation + timeline 内核 + snapping/placement + preview 视口合成器 + selection/commands`。
- 不搬 `services/storage(IndexedDB/OPFS 31 迁移)`，重接为 `Tauri SQLite` 适配层（`src/lib/local-store.ts`）。
- 不搬 `export/mediabunny`，保留 `src-tauri/src/preview.rs + jianying.rs` 本地交付。

## 8 批路线图

| 批 | 名称 | 内容 | 产出分支 | 验收 |
|---|---|---|---|---|
| P0 | 地基 | `rust/crates/time` + `src/wasm/media-timets` → 本仓 `src-tauri/src-tauri-opencut-time` 或纯 TS `media-time.ts`，`fps/utils` 精选移植 | `feat/opencut-p0` | `MediaTime` 单测 |
| P1 | 模型 | `timeline/types + element-utils + defaults/creation/tracks` 精选 + `params/registry` 裁剪 | `feat/opencut-p1` | 类型编译通过 |
| P2 | 指令/历史 | `commands` + `timeline/update-pipeline + ripple` 裁剪为基于 `TimelineVersion` 的 `operation_logs` 版本链 | `feat/opencut-p2` | `Ctrl+Z/Y` |
| P3 | 磁吸/放置 | `snapping/placement/group-move/resize + ruler-utils` 精选 | `feat/opencut-p3` | 磁吸 8px |
| P4 | 控制器 | `controllers(drag/resize/playhead/zoom) + hooks` 裁剪 | `feat/opencut-p4` | 拖/拉伸手感对齐 |
| P5 | 视口合成 | `preview/* + services/renderer/* + text/measure + guides` 裁剪为 `preview.rs` 的 Web 实时预览层 | `feat/opencut-p5` | 画布内编辑 |
| P6 | 面板 | `panels/assets/properties + subtitles` UI 精选 | `feat/opencut-p6` | Inspector 实时 |
| P7 | 存储桥 | `Tauri SQLite` 适配层，删除 `opencut` 临时目录 | `feat/opencut-p7` | `build + harness 绿` |
| P8 | 清理 | 简版 `StudioWorkspace.tsx` 下线 | `feat/opencut-p8` | `master` |

## 合并不了需自研

- `services/storage` → `src-tauri/src/db.rs` + `local-store.ts`
- `rust/crates/gpu/compositor/effects/masks` → `src-tauri/src/preview.rs` 保留
- `Next.js app/api + auth` → 已有 Tauri provider 体系

## 当前状态

P0 已试直拷失败，已清理 `src/opencut`，改走“精选移植”路径，从 `ruler-utils + media-time + time.rs` 开始逐文件裁剪搬运。

