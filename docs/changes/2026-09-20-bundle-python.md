# 2026-09-20：安装包捆绑 Python 与剪映草稿 SDK

## 触发范围

- `src-tauri/src/process.rs`
- `src-tauri/src/jianying.rs`
- `src-tauri/src/release_readiness.rs`
- `src-tauri/tauri.python.conf.json`
- `scripts/fetch-python.ps1`
- `scripts/verify-packaged-python.ps1`
- `scripts/run-tauri.mjs`
- `package.json`
- `.gitignore`
- `src-tauri/resources/python/NOTICE.md`

## 改动

Windows 安装包在 `npm run tauri:build` 时由 `scripts/run-tauri.mjs` 把基础资源、`tauri.ffmpeg.conf.json` 与 `tauri.python.conf.json` 合成一份清单再交给 CLI（多次 `--config` 会覆盖 `bundle.resources`）。捆绑官方 embeddable CPython 3.12.10，并预装 `pyJianYingDraft==0.3.0`、`pycapcut==0.0.3` 与 `MediaInfo.dll`。解释器 gitignored；构建前 `npm run python:fetch` 或 `npm run tauri:build` 自动拉取。Python 目录用 `resources/python` 整树拷贝，不用 glob，以便把 gitignored 的运行时文件打进安装包。

`python_program()` 解析顺序：`PYTHON_PATH` → 安装包/开发目录 `python.exe` → Windows 上回退 `py`（不传 `-3`）。启动适配器时把随包 FFmpeg 与 Python 目录插到子进程 PATH 前面，供封面抽帧与 `pymediainfo` 使用。Tesseract 仍不随包。

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

- `npm run python:fetch` 后 `python.exe` 能 `import pyJianYingDraft, pycapcut, pymediainfo`
- `npm run python:verify`：去掉 PATH 中的系统 python/py 后随包解释器仍能导入 SDK，并能 `shutil.which('ffmpeg')` 找到随包 FFmpeg
- `cargo check --manifest-path src-tauri/Cargo.toml`
- `npm run harness:check`
