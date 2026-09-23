# 2026-09-20：安装包捆绑 FFmpeg/FFprobe

## 触发范围

- `src-tauri/src/process.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/src/release_readiness.rs`
- `src-tauri/tauri.ffmpeg.conf.json`
- `scripts/fetch-ffmpeg.ps1`
- `scripts/run-tauri.mjs`
- `package.json`
- `.gitignore`
- `src-tauri/resources/ffmpeg/NOTICE.md`

## 改动

Windows 安装包在 `npm run tauri:build` 时合并 `tauri.ffmpeg.conf.json`，捆绑 Gyan `ffmpeg-8.1.2-full_build` 的 `ffmpeg.exe` / `ffprobe.exe`（含 `ass`/libass，供 preview 字幕）。二进制超过 GitHub 100MB，gitignored；构建前自动跑 `npm run ffmpeg:fetch`（官方 GitHub + gyan + ghfast 镜像，zip SHA-256 固定）。

运行时 `hidden_command("ffmpeg"|"ffprobe")` 解析顺序：`FFMPEG_PATH`/`FFPROBE_PATH` → 安装包/开发目录资源 → PATH。开发机未拉取二进制时仍可用系统 FFmpeg。Tesseract 与 Python 仍不随包。

## 同步文档

- `README.md`
- `docs/api.md`
- `docs/architecture.md`
- `docs/decisions.md`
- `docs/roadmap.md`
- `docs/codebase/STACK.md`
- `docs/codebase/INTEGRATIONS.md`
- `docs/codebase/CONCERNS.md`
- `TASKS.md`

## 验证

- `cargo check --manifest-path src-tauri/Cargo.toml`
- `npm run lint`（未改前端则跳过）
- `npm run harness:check`
- `npm run ffmpeg:fetch` 后确认 `ffmpeg -filters` 含 `ass`
