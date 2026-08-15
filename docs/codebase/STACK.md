# 技术栈

## 1）运行时概览

| 区域 | 当前值 | 证据 |
| --- | --- | --- |
| 桌面形态 | Windows 优先的 Tauri 2 本地桌面应用 | `src-tauri/tauri.conf.json`、`src-tauri/src/main.rs` |
| 前端语言 | TypeScript 6.0 + React TSX | `package.json`、`tsconfig.app.json` |
| 后端语言 | Rust 2021，声明最低 Rust 1.77.2 | `src-tauri/Cargo.toml` |
| 辅助适配器 | Python，调用 `pyJianYingDraft` | `src-tauri/scripts/create_jianying_draft.py` |
| 包管理 | npm lockfile v3；Cargo lockfile | `package-lock.json`、`src-tauri/Cargo.lock` |
| 当前开发机 | Node 22.23.1、npm 10.9.8、rustc/cargo 1.97.1 | 2026-08-15 终端版本检查 |

Node 的团队支持版本未在仓库中固定；不要把当前开发机版本理解为产品契约。[TODO] 增加 `.nvmrc` 或 `package.json.engines`。

## 2）生产框架与高影响依赖

| 依赖 | 版本 | 作用 | 证据 |
| --- | --- | --- | --- |
| React / React DOM | `^19.2.8` | 桌面 WebView 展示和交互 | `package.json` |
| Tauri API / dialog / opener | `^2.x` | IPC、原生文件选择、系统浏览器打开 | `package.json` |
| Tauri | `2.11.3` | Windows 壳、命令注册、事件、asset 协议 | `src-tauri/Cargo.toml`、`src-tauri/src/lib.rs` |
| rusqlite | `0.37` bundled | 本地 SQLite 与迁移 | `src-tauri/Cargo.toml`、`src-tauri/src/db.rs` |
| ureq | `2.12` | OAuth、自定义模型和 Jamendo HTTP 请求 | `src-tauri/Cargo.toml`、`src-tauri/src/provider.rs` |
| keyring | `3.6` windows-native | Windows Credential Manager | `src-tauri/Cargo.toml`、`src-tauri/src/oauth.rs` |
| serde / serde_json | `1.0` | Tauri、SQLite JSON 和 Provider schema | `src-tauri/Cargo.toml` |
| uuid | `1.18` | 项目、任务、版本、产物与审计 ID | `src-tauri/Cargo.toml` |

系统运行时依赖不由 Cargo/npm 安装：FFmpeg、FFprobe、Tesseract、Python、`pyJianYingDraft` 和 Jianying Pro。当前安装包没有捆绑这些依赖。

## 3）开发工具链

| 工具 | 用途 | 证据 |
| --- | --- | --- |
| Vite 8 | 开发服务器与前端生产构建 | `package.json`、`vite.config.ts` |
| TypeScript build mode | 编译期类型检查；当前未启用 `strict` 总开关 | `package.json`、`tsconfig.app.json` |
| Oxlint | React/TypeScript lint | `.oxlintrc.json` |
| rustfmt / Cargo test | Rust 格式和测试 | `src-tauri/Cargo.toml` |
| Python unittest | Jianying 适配器隔离测试 | `src-tauri/scripts/test_create_jianying_draft.py` |
| Node assert scripts | 架构预算、文档同步与 WebView 回归 | `scripts/`、`package.json` |

## 4）常用命令

```powershell
npm install
npm run tauri:dev
npm run lint
npm run build
npm run agent:check
npm run harness:test
npm run harness:check
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml
python -m unittest discover -s src-tauri/scripts -p "test_*.py"
```

`npm run dev` 只能检查浏览器展示，不能验证 SQLite、媒体工具或凭据。真实桌面验证需启动带 WebView2 CDP 端口的 Tauri 后运行 `npm run tauri:verify`。

## 5）环境与配置

- 应用配置：`src-tauri/tauri.conf.json`、`src-tauri/capabilities/default.json`。
- 编译配置：`tsconfig*.json`、`vite.config.ts`、`src-tauri/Cargo.toml`。
- 运行时读取：`TESSERACT_PATH`、`ProgramFiles`、`LOCALAPPDATA`、进程 `PATH`。
- 回归脚本读取：`TAURI_CDP_URL`、`TAURI_VERIFY_SCREENSHOT`。
- 仓库没有 `.env.example`，模型、OAuth 和 Jamendo 凭据不使用 `.env`。
- [TODO] 生产安装包的媒体/Python 运行时发现、版本兼容和安装失败说明尚未实现。

## 6）证据

- `package.json`
- `package-lock.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`
- `src-tauri/src/process.rs`
- `.harness/agent-context.json`
- `README.md`
