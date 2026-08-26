# Native 每轮权威状态快照

## 目标

让 NativeToolLoop 在每轮模型调用开始时主动提供当前本地项目的高层权威状态，使素材数量、分析进度、storyboard/timeline 版本、preview 与交付状态不再依赖关键词命中后的观察工具 nudge，同时保留观察工具作为细节读取手段。

## 实现

- 新增 `agentloop/snapshot.rs`，从当前 project/editing task 作用域聚合任务 brief 与最近终态、素材 kind/技术/视觉/健康计数、最新 storyboard 和其 timeline 的版本与数量、磁盘 preview、Jianying 注册及模型/配音/Jamendo 配置状态。
- 快照固定首行与字段顺序，总长最多 1200 字符。任务 brief 最多 180 字符且使用保守字符集；若含路径、文件名、ASCII 字母、字段分隔符或 UUID 则整体隐藏，避免隐私泄漏和伪造状态字段。输出不包含数据库 ID、本机路径、素材文件名、用户备注、OCR/视觉证据、会话原文、Base URL、模型名或凭据值。
- Provider input 固定为系统提示、唯一快照、历史、当前用户消息。初始构建失败返回安全固定码，不静默省略快照。
- 非观察写工具返回 `ok`、`queued` 或 `needs_confirmation` 后，在下一次 Provider 请求前重读并原位替换快照；刷新失败封闭终止，不重放工具。
- 输入预算保护快照、当前用户消息及最近 function_call/function_call_output 对。成功快照在 RunReceipt 中记为来源 `state_snapshot` 的成功观察；原观察关键词和 nudge 分支未删除。
- Credential Manager 读取仍只发生在 `custom_api.rs`、`oauth.rs` 和 `music_provider.rs`。新内部接口只返回配置布尔值；明确未配置与读取失败分开，后者使快照失败封闭。

## 保持不变

- 不新增或修改 Tauri 命令、Agent 工具、前端类型、SQLite schema 或依赖。
- 不修改 `request_requires_project_observation` 词表、不拆 `native_policy.rs`、不做内存缓存或自然语言事实核对。
- `get_edit_status` 的作用域、上一任务终态和磁盘 preview 深度不变；决策级工具硬门不被快照替代。
- 完成门发现既有契约 fixture 和白名单已包含 23 个工具，但集成测试仍硬编码 21；仅同步该计数断言为 23，名称逐项对账和工具目录本身均未改变。

## 测试

- 快照 fixture 覆盖路径、文件名、UUID、OCR 正文不泄漏，超长/敏感 brief 隐藏或截断，总长不超过 1200 字符。
- 产物 fixture 覆盖 storyboard/timeline 版本与轨道计数、真实 preview 文件和 Jianying 注册状态，不输出内部 ID。
- Native input fixture 覆盖唯一快照的位置、初始构建失败封闭、写工具后下一次请求看到新版本且仍只有一条快照。
- 16k 裁剪回归覆盖快照与最近 call/output 对同时保留；项目事实直接文本回答在 `state_snapshot` 收据在场时不触发 nudge，原无观察收据的 nudge 测试继续保留。

## 验证状态

- [x] `cargo fmt --check`、`cargo check`、219 个 Rust 库测试及 2 个契约测试通过。
- [x] `npm run lint`、`npm run build`、`npm run agent:check`、`npm run harness:test`、`npm run harness:check` 与分支检查通过；14 个 Python 测试及 `git diff --check` 通过。
- [x] 高风险 Native 上下文与凭据边界经独立 Agent 审查；文件名/字段注入、Provider 布尔边界和视觉计数问题修复后复核无剩余阻塞。

## 同步文档

- `AGENTS.md`
- `README.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/harness.md`
- `docs/codebase/INTEGRATIONS.md`
