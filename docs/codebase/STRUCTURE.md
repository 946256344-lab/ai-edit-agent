# 代码库结构

## 1）顶层地图

| 路径 | 职责 | 证据 |
| --- | --- | --- |
| `src/` | React 展示、领域 controller、Tauri TypeScript 桥 | `src/main.tsx`、`src/App.tsx` |
| `src-tauri/src/` | 本地可信边界：命令、SQLite、Agent、媒体和交付 | `src-tauri/src/lib.rs` |
| `src-tauri/scripts/` | Rust 调用的 Jianying Python 适配器及测试 | `src-tauri/scripts/create_jianying_draft.py` |
| `src-tauri/tests/` | Rust 集成契约和版本化 Agent fixture | `src-tauri/tests/agent_contract_assets.rs` |
| `scripts/` | 开发期分支、架构、文档和真实 WebView 检查 | `package.json` |
| `.harness/` | 机器可读分支策略、架构预算、Agent 上下文清单与文档同步策略 | `.harness/branch-policy.json`、`.harness/architecture-budgets.json`、`.harness/agent-context.json` |
| `docs/` | 长期架构、API、ADR、路线图、审计与变更记录 | `docs/architecture.md` |
| `AGENTS.md`、`src/AGENTS.md`、`src-tauri/src/AGENTS.md` | 编码 Agent 的全局入口与目录级就近约束 | 三份指令文件 |
| `CONTRIBUTING.md`、`CLAUDE.md`、`.cursor/rules/`、`opencode.json` | 唯一协作流程与各工具薄入口，不分配固定职责 | `.harness/agent-context.json` |
| `.github/pull_request_template.md` | PR 目标、边界、风险和验证证据模板 | `CONTRIBUTING.md` |
| `public/`、`src/assets/` | 静态前端资源 | `vite.config.ts`、`src/main.tsx` |

`dist/`、`src-tauri/target*/`、`src-tauri/gen/` 是构建或生成产物，不是源架构。自定义 `target-mvp-verify/` 会干扰通用扫描，应在度量时排除。

## 2）入口点与启动顺序

1. `src-tauri/src/main.rs` 只调用 `app_lib::run()`。
2. `src-tauri/src/lib.rs` 安装 log/dialog/opener 插件并注册稳定 Tauri 命令表。
3. `src/main.tsx` 将 `<App />` 挂载到 WebView。
4. `src/App.tsx` 调用 `initializeLocalStore()`，再加载首个项目和剪辑任务。
5. `projects::initialize_local_store` 执行中断任务恢复、素材分析恢复和 Jianying 注册恢复。
6. `src-tauri/scripts/create_jianying_draft.py` 不是独立入口，只能由 `jianying.rs` 以版本化 JSON handoff 调用。

## 3）前端边界

```text
src/main.tsx
  -> App.tsx                    顶层项目/任务/conversation 组合
     -> hooks/                  领域状态与异步副作用协调
     -> components/             无 Tauri 访问的展示工作区
     -> lib/local-store.ts      唯一 invoke 类型桥
```

| 边界 | 可以拥有 | 不应拥有 |
| --- | --- | --- |
| `App.tsx` | 当前项目/任务、消息入口、工作区组合 | Provider、素材轮询、产物交付、Agent 终态算法 |
| `hooks/use*Controller.ts` | 一个领域的状态和命名动作 | JSX 布局、Rust/SQLite 细节 |
| `components/` | `model/actions` 展示、局部纯 UI 状态 | `invoke`、文件系统、跨领域持久化 |
| `lib/local-store.ts` | camelCase 类型和命名 Tauri wrapper | 业务工作流、React 状态 |
| `lib/agent-tools.ts` | IDE 目标工具类型镜像 | 执行白名单；真实白名单在 Rust/fixture |

## 4）Rust 模块地图

| 模块 | 当前职责 |
| --- | --- |
| `lib.rs` | 插件初始化与 Tauri 命令注册 |
| `projects.rs` | 项目、任务、conversation、消息、启动恢复 |
| `taskrouter.rs` | 项目内任务归属、快照、pending route、一次性 receipt |
| `agent.rs` | conversation route 入口、异步 run、原子终态提交 |
| `agentloop.rs` | Conversation Router、prompt、状态快照、有界循环与技能派发 |
| `agentloop/policy.rs` | 工具白名单、负向约束、目标解析、真实产物完成门与固定降级文案 |
| `assets.rs` | 导入、分析、目录、搜索、健康、重链路、收集 |
| `storyboard.rs` | 证据候选、模型提案、校验和版本 |
| `timeline.rs` | 时间线版本、镜头/文本/音乐编辑和查询 |
| `preview.rs` | FFmpeg 渲染、文本/音乐合成、质量检查 |
| `jianying.rs` | 新草稿创建和延迟注册 |
| `provider.rs` | Provider 选择、传输转换、优先级和熔断 |
| `oauth.rs`、`custom_api.rs`、`music_provider.rs` | 外部集成和凭据 |
| `db.rs`、`models.rs`、`audit.rs`、`process.rs` | 数据库、边界类型、审计、外部进程基础设施 |

## 5）命名与组织规则

- React 组件使用 PascalCase 文件；hook 使用 `useXxxController.ts`。
- Rust 文件和函数使用 snake_case，Tauri 命令名与函数名一致。
- TypeScript 没有 path alias，使用相对导入；纯类型使用 `import type`。
- Rust 领域模块直接使用 `open_connection` 和 SQL，目前没有 repository 目录或 DI 容器。
- 测试主要与 Rust 源文件共置；跨模块契约位于 `src-tauri/tests/`。

## 6）建议的 IDE 阅读顺序

1. 根 `AGENTS.md`、`TASKS.md` 当前窗口与目标目录 `AGENTS.md`：确认本轮边界。
2. `src-tauri/src/lib.rs`：总模块和命令面。
3. `src/App.tsx`：前端如何选择项目、任务和工作区。
4. `src/lib/local-store.ts`：前后端方法映射。
5. `taskrouter.rs` → `agent.rs` → `agentloop.rs`：一次自然语言请求。
6. `assets.rs` → `storyboard.rs` → `timeline.rs` → `preview.rs` → `jianying.rs`：一次产物链路。
7. `db.rs` 与 `models.rs`：持久化事实和序列化形状。

## 7）证据

- `src-tauri/src/lib.rs`
- `src/App.tsx`
- `src/hooks/`
- `src/lib/local-store.ts`
- `.harness/architecture-budgets.json`
- `.harness/agent-context.json`
- `docs/architecture.md`
