# 代码库关注点

## 1）优先风险

| 严重度 | 关注点 | 证据 | 影响 | 建议动作 |
| --- | --- | --- | --- | --- |
| 高 | `agentloop.rs` 3599 行，仍混合路由、prompt、快照、循环、技能派发和测试；纯 policy 已提取 | `src-tauri/src/agentloop.rs`、`agentloop/policy.rs` | executor 改动仍可能污染路由和状态恢复 | policy 保持无副作用；后续抽 router/state，最后搬 executor |
| 高 | `assets.rs` 4114 行，混合六类领域职责和两类 worker | `src-tauri/src/assets.rs` | 导入、目录、分析或健康变更互相污染 | 按 import/technical/visual/library/health/metadata 拆子模块 |
| 高 | 完整 Agent 多步 fixture 不能执行 | `src-tauri/tests/fixtures/README.md` | prompt/schema/状态组合回归只能靠局部测试和实机发现 | 增加 scripted decision seam 与临时 SQLite runner |
| 高 | 生产安装包不供应媒体/Python 运行时 | `README.md`、`src-tauri/tauri.conf.json` | 开发机可用不代表用户机器可用 | 先做运行时探测矩阵，再决定捆绑或安装引导 |
| 中 | 没有 CI、远端分支保护和覆盖率阈值 | `.githooks/pre-commit`、`.harness/branch-policy.json`，且仓库无 `.github/workflows/` | 本地 hook 可被跳过，回归证据不统一 | 在 GitHub 启用 master 保护，并建立 Windows lint/build/Rust/Python/harness CI |
| 中 | `timeline.rs` 1848 行，文本、音乐、镜头编辑共存 | `src-tauri/src/timeline.rs` | 下一轮轨道能力会继续膨胀 | 在 Agent/assets 稳定后按 editing/text/music/repository 拆 |
| 中 | 前端入口仍接近预算 | `App.tsx` 515 行；artifact controller 363 行 | 新功能可能再次把 task 和产物职责混回 | 下一次功能前拆 conversation/task controller 与 artifact 子域 |

## 2）技术债务

| 债务 | 现状原因 | 位置 | 忽略风险 | 修复方向 |
| --- | --- | --- | --- | --- |
| 领域 SQL 分散 | 本地应用直接使用 rusqlite，早期速度优先 | 多个 `src-tauri/src/*.rs` | 拆模块时破坏跨表事务 | 不先造通用 repository；先提取每个领域的 scoped query/transaction 函数 |
| 手写 TS 工具镜像 | `agent-tools.ts` 不参与运行时 | `src/lib/agent-tools.ts` | 再次与 Rust 白名单漂移 | 待确认生成或删除策略，见问题 3 |
| `local-store.ts` 平铺 562 行 | 一个 bridge 集中全部命令 | `src/lib/local-store.ts` | 类型和命令查找变慢、跨域改动冲突 | 按 projects/assets/artifacts/agent/provider 分文件，由单一 index 汇总 |
| TypeScript 未开启 `strict` 总开关 | 当前只启用了若干局部严格选项 | `tsconfig.app.json` | null、函数参数方差等类型缺口不能由现有 build 完整发现 | 单独建立迁移基线后逐项开启，不在一次提交中制造大量无关修复 |
| `PartiallyDone` 分支未构造 | 终态枚举保留了未使用变体 | `agentloop.rs`、`cargo check` | 每次 Rust 构建产生 dead-code warning，可能遮蔽新增警告 | 确认不再需要后删除，或在真实部分完成路径中明确使用并补测试 |
| ADR 编号/状态不完全整齐 | 连续恢复期频繁追加 | `docs/decisions.md` | 新成员难判断取代关系 | 单独做文档索引，不重写历史 ADR 正文 |
| Agent 语义理解不可机器证明 | Markdown 和正则检查只能提供入口与边界 | 三份 `AGENTS.md`、`.harness/agent-context.json` | Agent 可能满足字面检查却误解产品目标 | 保持当前窗口短小、重要规则下沉 Rust/测试、用独立审查与真实验收闭环 |
| 扫描包含自定义 target | 构建目录名不等于默认 `target/` | `src-tauri/target-mvp-verify/` | 代码度量被二进制污染 | 更新扫描排除或把 target 移至统一构建缓存 |

## 3）安全关注

| 风险 | 类别 | 证据 | 当前缓解 | 缺口 |
| --- | --- | --- | --- | --- |
| 自定义 Provider URL 只校验非空 | OWASP A10/配置风险 | `custom_api::validate_input` | 用户显式配置、凭据在 Credential Manager | 待确认 HTTP/localhost 边界，见问题 2 |
| 实验性 OAuth 依赖非官方稳定契约 | N/A | `oauth.rs`、`provider.rs` | 明确标为实验性、失败封闭 | 上游 URL/scope/model 可变化，仍需真实刷新验证 |
| Jamendo 凭据缺失与读取失败未区分 | 配置/诊断 | `music_provider.rs` | 保存失败返回 `failed`，不泄露凭据 | 状态读取把所有 keyring 错误都映射为 `disconnected`，应增加安全原因状态 |
| 外部进程终止不能保证完整 | N/A | `process.rs` | 超时、`taskkill /T /F`、不无限等待 | 终止失败后缺少系统级孤儿进程观测 |
| preview/Jianying 部分同步进程无超时 | 可用性 | `preview.rs`、`jianying.rs` | 统一无窗口创建，素材分析阶段已有超时 | FFmpeg/Python 异常挂起时可能阻塞交付；应复用可终止的超时执行器 |
| 本地路径可能进入内部错误 | OWASP A09/隐私 | 多个 `Result<T,String>` | UI 固定文案、审计安全码、禁止模型原文日志 | 新日志/诊断必须持续审查，不应输出底层 error 原文 |
| Google Fonts 产生网络请求 | 隐私/供应链 | `src/index.css` | CSP 仅允许指定字体域 | 离线/严格本地产品是否应内置字体尚未决策 |

## 4）性能与扩展关注

| 关注点 | 证据 | 当前表现 | 扩展风险 | 建议 |
| --- | --- | --- | --- | --- |
| 轮询多个本地投影 | assets 1.5s、Agent 1.2s、health 2s、OAuth 2s | 当前单项目可用 | 更大项目增加重复 SQL | 保持有界查询；未来用状态事件唤醒但保留轮询恢复 |
| 多数持久化领域命令按调用新开 SQLite connection 并 migrate | `db::open_connection` 与各领域命令 | WAL + busy timeout 已缓解锁 | 高频命令重复迁移检查 | 度量后再考虑受控连接管理，不在无证据时引入全局池 |
| 视觉/技术 worker 位于巨型模块 | `assets.rs` atomics/thread | 已有 2 技术 worker、1 视觉 worker | 新分析类型使全局状态复杂 | 拆 worker coordinator 与纯分析 stage |
| Preview 顺序渲染 | `preview.rs` | 适合当前短视频 | 更长/多轨会放大临时文件和耗时 | 加阶段耗时、磁盘预算和取消策略后再并行化 |

## 5）高变更/脆弱区域

| 区域 | 脆弱原因 | 变更信号 | 安全修改策略 |
| --- | --- | --- | --- |
| `App.tsx` | 路由、消息和工作区历史集中 | 最近 20 次提交中出现 8 次 | 保持预算，新增 task 行为先抽 controller |
| `assets.rs` | 媒体/路径/DB/worker 交叉 | 最近提交多次修复性能与目录 | 先写/保留纯路径和队列测试，再搬代码 |
| `agentloop.rs` / `agent.rs` | 权限、模型、终态事实耦合 | 多个连续 Agent ADR | 所有改动跑 fixture、事务和负向约束回归 |
| 长期文档 | 恢复期高频同步 | TASKS/architecture/decisions 各 10 次 | 新增索引与证据，不覆盖历史事实 |

## 6）后端拆分顺序

1. [已完成] `agentloop/policy.rs`：已搬工具常量、`RequestToolPolicy`、goal/完成门纯函数；行为测试继续从父模块覆盖公开运行语义。
2. `assets/library.rs`：只搬安全路径/目录投影和只读搜索，保持 Tauri 命令在原模块转发。
3. `agentloop/router.rs` 与 `state.rs`：保留公开 crate 函数签名。
4. `assets/technical.rs` 与 `visual.rs`：每次只迁移一个 worker，验证启动恢复和熔断。
5. `agentloop/executor.rs`：最后移动 `apply_skill/run_step`，因为它连接所有副作用。
6. 再处理 timeline、local-store 和 App；不同时拆前后端同一契约。

## 7）`[ASK USER]` 问题

1. [ASK USER] 自定义模型是否必须支持局域网 `http://`/localhost？这决定 Base URL 应强制 HTTPS，还是允许显式不安全本地连接。
2. [ASK USER] `src/lib/agent-tools.ts` 未来要成为可发布 SDK 契约，还是仅作为 IDE 镜像？建议由版本化 Rust fixture 自动生成，避免第三份手工白名单。

## 8）意图与现实偏差

| 长期意图 | 当前现实 | 处理方向 |
| --- | --- | --- |
| 领域模块清晰、`App.tsx` 聚焦组合 | 前端已明显分层，Agent 纯 policy 已独立；父 `agentloop.rs` / `assets.rs` 仍是物理热点 | 下一步 `assets/library`，再按第 6 节交替渐进搬迁 |
| “严格 TypeScript 检查” | build 有多项严格选项，但 `tsconfig.app.json` 未开启 `strict` | 先建立迁移基线，再决定启用顺序 |
| 版本化 Agent fixture 防止工具/场景回归 | 工具目录契约可执行，完整 scripted Provider 多步 runner 尚未实现 | 增加可注入 decision seam 与临时 SQLite runner |
| 另一编码 Agent 可从文档接手 | 分层入口和硬门已建立，但未提交工作、隐含用户意图和真实桌面状态仍不能只靠文档恢复 | 交接时保持 Git 状态、当前任务窗口、变更记录和验收证据准确 |
| Windows 本地产品可独立安装运行 | 当前安装包未捆绑 FFmpeg、Tesseract、Python/Jianying adapter 运行时 | 先完成探测矩阵，再选择捆绑或安装引导 |

## 9）证据

- 2026-08-15 终端扫描输出（目录树、代码度量、最近 20 次提交与高变更文件）
- `.harness/architecture-budgets.json`
- `src-tauri/src/agentloop.rs`
- `src-tauri/src/agentloop/policy.rs`
- `src-tauri/src/assets.rs`
- `src-tauri/tests/fixtures/README.md`
- `src/lib/local-store.ts`
- `git log -20`（2026-08-15 扫描）
- `.harness/agent-context.json`
