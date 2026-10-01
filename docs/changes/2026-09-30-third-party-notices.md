# 第三方许可声明与设置入口

## 结果与边界

- 根 `THIRD_PARTY_NOTICES.md` 汇总固定组件版本、许可、版权归属或待确认状态、随包全文位置及 FFmpeg 记录的上游源码获取链接。
- `src-tauri/resources/third-party/` 包含收集的组件全文、Python 实际分发快照、npm 生产依赖与 Windows Cargo runtime 清单，`ALL.txt` 汇总供应用读取。许可未声明、版权未查明和配套 DLL 条款缺失保留「待确认」。这是披露与入口实现，不是合规结论。
- 设置弹窗新增中英文「第三方许可声明 / Third-party notices」，展开后调用固定文件只读命令，显示可滚动、可选择的纯文本；失败安全提示并可重试。
- `src-tauri/tauri.conf.json` 基础资源列表增加 `resources/third-party`，沿用 `scripts/run-tauri.mjs` 原有数组追加合并机制，不替换其他 runtime 资源。
- 正式构建入口在 runtime 准备后收集本次实际 Python 分发快照并离线生成依赖声明，失败则中止，避免安装包使用过时的间接依赖声明。

## 同步文档

- `docs/api.md`：固定文件异步读取命令、返回类型与失败边界。
- `THIRD_PARTY_NOTICES.md`：组件清单、源码链接、生成方式及未确认项。
- `docs/release-checklist.md` §1.5：改为部分实现，保留 P0 未关闭和桌面 / 许可缺口。

## 生成与证据

`node scripts/collect-runtime-notices.mjs --runtime-root <已获取资源目录>` 只读已有 runtime，输出仅在本 worktree。收集 11 份组件文本、9 个 Python distribution 和 51 个 Tesseract DLL 名称。本次输入为主仓库已获取资源目录，未修改该目录；间接 Python 依赖和 pip 版本不是 fetch 脚本固定值，明确标为实际快照。

`npm run notices:generate` 离线解析前端非 type 引用，以实际引用的生产包为根，遍历 npm 锁文件依赖并收集已安装许可文本；排除 dev / devOptional、`@types/*` 与没有运行时引用的 `@moviemasher/moviemasher.js`、`jassub`，保留 manifest 依赖闭包（不宣称已精确识别 tree-shaking 后的字节）。Cargo 使用 `metadata --offline --locked --filter-platform x86_64-pc-windows-msvc`，只遍历 normal 依赖、排除 build/dev-only 和 proc-macro 编译工具。本次为 98 个 npm 生产包与 320 个 Windows runtime crates。声明详细记录了生成口径与重新生成方式。

包内没有全文的 13 个 Cargo crates 用 20 份上游补充文本补齐，来源与 SHA-256 见 `supplemental-sources.json`：Cargo 按发布包记录的 Git 提交获取，不使用最新许可替代固定版本；MPL-2.0 按 Mozilla 公布文本提供。离线重新生成后 npm/Cargo 缺失全文均为 0，仍保留未确认版权归属等真实状态。

## 发布仍需确认

- 用户在正式安装包设置里展开入口，确认英文/中文文案、全文加载、滚动与文本选择；检查安装资源目录包含汇总及全文。此次按任务要求不启动应用、不启动开发服务器、不构建安装包。
- pycapcut 0.0.3 元数据没有许可证且包内未附全文，不能据其他 SDK 的许可推断；需要明确授权依据。
- Tesseract 抓取脚本只保留引擎许可证，51 个随包 DLL 的版本、许可全文和版权未齐；CPython 附带 VC runtime 的再分发条款也需核对。
- FFmpeg full_build 的确切对应源码（外部静态库、构建脚本、补丁）与 GPL 分发方式，以及 H.264/HEVC 专利事项交由用户及法务确认；源码上游链接不视为已满足全部义务。
- 按已获取 FFmpeg 的 `-version` 记录真实构建配置并逐项列出外部库标志；外部库的独立版本、版权与条款仍待确认。该命令只查询媒体 runtime 版本，此次没有启动 Voycut 或开发服务器。
- 本文不修改产品决策、任务清单或其他功能，不推送、不合并。

## 验证

- `npm run lint`：通过（退出码 0）。
- `cargo check --manifest-path src-tauri/Cargo.toml`：通过（退出码 0；6 条既有 dead_code 警告来自 storyboard 模块）。
- `npm run harness:check`：通过（架构预算、命令契约、文档同步与 i18n 检查）。
- `node node_modules/typescript/bin/tsc -b --pretty false`：通过。
- `npm run notices:generate`：离线通过；98 个 npm 包、320 个 Cargo crates，缺失许可全文均为 0。连续生成 `ALL.txt` 的 SHA-256 一致。
- `node --check`：4 个新增 / 修改的 Node 脚本语法通过。
- 静态重放原资源数组合并：普通配置 26 项、完整模型配置 29 项，第三方声明及原 runtime 项均保留。
- `git diff --check` 与 `git diff --cached --check`：通过。

静态通过不代表桌面或安装包验收通过；不关闭 §1.5 的发布 P0。
