# 文档改进与 timeline v6 只读审计

## 结果

- `TASKS.md`：将 2026-08-14 及更早历史条目归档至 `docs/changes/TASK_HISTORY.md`；在"待决问题"中新增两条阻断项（自定义 API `http://` 支持、agent-tools SDK 定性），并引用 `CONCERNS.md §7`。
- `docs/architecture.md`：在"## 状态"后新增 16 行能力速查表；在"当前实现细节"中为素材库、媒体分析队列、视觉分析、Provider 认证、Agent 循环与请求策略、会话路由与持久化、文本轨七个小节增加三级标题；修正错误引用 `docs/codebase/ARCHITECTURE.md` → `docs/codebase/CONCERNS.md`；消除后端模块拆分路线重复描述。
- `docs/codebase/CONCERNS.md`：§7 两条 `[ASK USER]` 问题已分别标注"已列入 TASKS.md 待决问题"及所阻断的工程任务。
- `CONTRIBUTING.md`：将 worktree 示例中的占位符"素材目录"改为 `<task-slug>`。
- `src-tauri/src/AGENTS.md`：外部边界超时债务描述从模糊文字改为指向 `docs/codebase/CONCERNS.md §3`。
- `docs/changes/TASK_HISTORY.md`（新增）：保存 2026-08-14 的全部历史执行条目。
- `docs/audits/2026-08-15-timeline-v6-media-fact-audit.md`（新增）：对 timeline v6 进行只读媒体事实审计。确认 v5→v6 变更仅 shot2 缩短 500 ms；全部 8 个 asset ready/online/未 excluded；时间线 0–31,689 ms 连续；source 边界无越界。记录系统性问题：`preview.rs::render_timeline_clip` 未将 `source_end_ms` 传给 FFmpeg，shot1/shot3 实际可用素材短于 timeline slot，shots 4–8 的 source_end 约束静默失效。未修改任何数据库记录、timeline、preview 或用户数据。

## 同步文档

- `AGENTS.md`
- `CONTRIBUTING.md`
- `README.md`
- `docs/architecture.md`
- `docs/decisions.md`
- `docs/harness.md`
- `TASKS.md`
