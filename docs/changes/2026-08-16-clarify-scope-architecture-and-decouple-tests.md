# 2026-08-16：明确作用域架构并修复测试错误放置

## 问题

1. **文档缺口**：`docs/architecture.md` 没有明确说明会话（conversation）只是对话容器，不拥有产物。产物（storyboard、timeline、preview、Jianying draft）归属剪辑任务而非会话，但这一关键事实没有以 ASCII 图和文字形式沉淀，容易被 Agent 或新开发者误解为产物属于某个特定会话。

2. **测试错误放置**：上一轮修复 `timeline.rs::select_timeline_candidate` 时，将多版本回归测试放入了 `agentloop.rs` 的测试模块，并用 `#[rustfmt::skip]` 压缩格式来绕过 `agentloop.rs` 的行数预算。行数预算的目的是强制解耦，不是鼓励格式压缩绕过。

## 变更

### `docs/architecture.md`
在"作用域架构"小节添加 ASCII 图和文字说明：

```
Project (项目)
├── Assets (素材，项目级复用)
└── Editing Tasks (剪辑任务，创作目标单元)
    ├── Conversations (会话，对话容器；一个任务可有多个会话)
    │   └── Messages
    ├── Storyboard Versions (故事板，直接归任务)
    ├── Timeline Versions (时间线，通过 storyboard 归任务)
    └── Previews / Jianying Drafts (基于 timeline，归任务)
```

会话只是对话容器，不拥有产物。产物查询和创建只需 `(project_id, editing_task_id)`，不依赖 `conversation_id`。

### `src-tauri/src/timeline.rs`
将多版本回归测试移入正确归属模块。新增：

```rust
#[test]
fn select_timeline_candidate_picks_latest_when_multiple_versions_exist()
```

行数从 1848 增至 1875（+27），系测试迁回正确模块，不是功能增长。

### `src-tauri/src/agentloop.rs`
- 移除 `#[rustfmt::skip]`，将 `delivery_tools_require_a_scoped_timeline_instead_of_creating_one` 展开为标准格式
- 删除跨模块的多版本断言（已移入 `timeline.rs`），保留 ID 匹配、单条无 ID、空候选三个本模块关心的行为断言
- 行数保持 3599（恰好在预算内）

### `.harness/architecture-budgets.json`
- `src-tauri/src/timeline.rs` maxLines 从 1848 放宽至 1875，以接受正确放置的测试
- 此放宽需在 commit message 中注明理由，ratchet 才会接受

### `src-tauri/src/AGENTS.md`
新增"测试放置与预算"小节，明确：
- 单元测试必须放在被测函数所在模块
- 架构预算不得用 `#[rustfmt::skip]` 或格式压缩绕过
- 合理放宽需在 commit message 写明理由

## 影响范围

- 不修改公开命令、SQLite schema、工具白名单或用户数据
- `timeline.rs` 测试数量：+1（多版本选取回归）
- `agentloop.rs` 测试数量：不变（断言重组，未删减覆盖范围）

## 完成门

- 130 个 Rust 库测试通过（含新增测试）
- `cargo fmt --check` 通过
- `harness:check` 在 commit 后通过（ratchet 以新提交为基线）
