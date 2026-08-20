# 外部集成

## 1）集成清单

| 系统 | 类型 | 用途 | 鉴权 | 关键性 | 证据 |
| --- | --- | --- | --- | --- | --- |
| SQLite | 本地数据库 | 项目、任务、消息、素材、版本、审计 | 本机文件权限 | 高 | `src-tauri/src/db.rs` |
| Windows Credential Manager | secrets store | OAuth、自定义 API、Jamendo 凭据 | Windows 用户上下文 | 高 | `oauth.rs`、`custom_api.rs`、`music_provider.rs` |
| 实验性 OpenCode 兼容 OAuth | OAuth/HTTP API | 模型访问 | loopback PKCE + token | 高、实验性 | `oauth.rs`、`provider.rs` |
| 自定义 OpenAI-compatible API | HTTP API | 主模型/粗视觉模型 | Bearer API key | 高 | `custom_api.rs`、`provider.rs` |
| Jamendo | HTTP API + download | 搜索/下载 CC0、CC-BY 音乐 | client ID | 中 | `music_provider.rs` |
| FFmpeg / FFprobe | 本机进程 | 分析、抽帧、preview、质量检查 | 无 | 高 | `assets.rs`、`preview.rs` |
| Tesseract | 本机进程 | OCR | 无 | 中 | `assets.rs` |
| Python + pyJianYingDraft | 本机适配器 | Jianying draft 文件生成 | 无 | 高、实验性 | `jianying.rs`、`create_jianying_draft.py` |
| Jianying Pro | 本地应用/文件格式 | 草稿注册和后续人工编辑 | 本机用户 | 高、单向 | `jianying.rs` |
| Google Fonts | WebView 静态资源 | UI 字体 | 无 | 低 | `src/index.css`、`tauri.conf.json` |

## 2）数据存储

| 存储 | 作用 | 访问层 | 主要风险 | 证据 |
| --- | --- | --- | --- | --- |
| `assembly-video-agent.sqlite3` | local project 事实 | Rust `open_connection` + 各领域 SQL | 多模块事务耦合 | `db.rs` |
| 应用数据目录 | 缩略图、关键帧、preview、下载音频 | `assets`、`preview`、Tauri asset 协议 | 磁盘增长与运行时迁移 | `tauri.conf.json` |
| 源媒体路径 | 只保存引用，不复制/改写 | `assets.rs` | 盘符/网络盘失联、隐私 | `docs/api.md` |
| Windows Credential Manager | API key/token/client ID | `keyring::Entry` | 损坏时必须失败封闭 | `custom_api.rs`、`oauth.rs` |
| Jianying 草稿目录/注册表 | 单向交付物 | Rust + Python adapter | 版本兼容、注册并发 | `jianying.rs` |

SQLite 每次打开启用 5 秒 busy timeout、WAL、`synchronous=NORMAL` 和 foreign keys；schema 当前为 v14，迁移只增不删。

## 3）凭据与数据边界

- 凭据不进入 SQLite、localStorage、日志、Agent 工具结果或文档示例。
- 自定义 API 配置整体保存到 Credential Manager；读取错误不会静默回退 OAuth。
- 模型只接收精简 prompt、证据文本和低分辨率派生帧，不接收原始媒体或本机路径。
- Jamendo 工具只接受 API 明示允许下载且为 CC0/CC-BY 的曲目，并保留 CC-BY attribution。
- 自定义 Base URL 当前只校验非空，没有 scheme/TLS 约束；是否允许局域网 HTTP Provider 是待确认的产品边界，见 `CONCERNS.md` 问题 2。

## 4）可靠性与失败行为

- NativeToolLoop 最多 10 步、300 秒总预算，每个模型步骤最多 120 秒；瞬时 Provider 传输失败的最多三次尝试共享同一模型步骤剩余预算，每次 HTTP 只用剩余预算除以剩余次数的份额，不扩大总预算。
- 粗视觉连续三次失败后熔断 60 秒；交互请求优先于未开始的视觉请求。
- 素材分析中的 FFprobe/FFmpeg/Tesseract 使用硬超时和 Windows 子进程树终止请求；preview 与 Jianying 的部分同步交付进程仍无超时，见 `CONCERNS.md`。
- Agent 工具失败不无限自动重试；Provider 瞬时传输重试只重发当前模型 payload，不重放本地工具；中断 run 转 `needs_review`。
- 素材技术分析和视觉分析有独立恢复 worker；Jianying 注册可延迟重试。
- Tauri 事件不是事实存储；Agent 完成依靠 SQLite + 轮询恢复。
- 当前没有远程消息队列、服务网格、API gateway 或云数据库。

## 5）可观测性

- `tauri-plugin-log` 记录固定阶段日志。
- `agent_diagnostics` 只保存阶段、长度、耗时和安全错误码。
- debug 构建可用 `NATIVE_PROVIDER_FULL_TRACE=1` 将 NativeToolLoop 每次真实 HTTP 尝试的完整 wire request/response 追加到 `src-tauri/target/native-provider-full-trace.jsonl`；成功与 HTTP 错误正文都保留，写入前精确遮蔽当前 Provider 凭据、账户标识和自定义 Base URL，未收到响应时不伪造 output。它不进入普通产品日志、SQLite、localStorage、Tauri 命令或前端，release 构建强制关闭，且记录不含 HTTP 请求头。
- `agent_run_steps` 保存 payload-free 步骤，`operation_logs` 保存产物副作用摘要。
- 没有 APM、分布式 tracing、metrics exporter 或集中日志系统。[TODO]
- 媒体 worker 的队列深度可从本地任务状态展示，但没有长期吞吐/失败率指标。[TODO]

## 6）证据

- `src-tauri/src/provider.rs`
- `src-tauri/src/custom_api.rs`
- `src-tauri/src/oauth.rs`
- `src-tauri/src/music_provider.rs`
- `src-tauri/src/process.rs`
- `src-tauri/tauri.conf.json`
