# Plan: 明确作用域架构并解耦 agentloop.rs 测试

## Context

用户提出两个根本性架构问题：

1. **会话与产物的作用域混淆**：会话（conversation）看起来没有真正隔离。除了素材归项目所有，storyboard、timeline、preview 等产物实际上都只归属剪辑任务（editing_task），而不是会话。但文档和代码没有清晰表述这一点。

2. **测试放置违背解耦原则**：刚才为修复 `timeline.rs::select_timeline_candidate` 添加的回归测试被放在了 `agentloop.rs` 的测试模块中，并用 `#[rustfmt::skip]` 压缩格式以绕过行数预算。这违背了架构预算"请拆分职责"的本意——预算限制是为了强制解耦，而不是用格式技巧绕过。

## Phase 1: 确认当前架构事实

从数据库 schema (schema v7+) 看：

```sql
-- 项目 > 剪辑任务 > 会话，三层作用域
CREATE TABLE projects (id, name, ...);
CREATE TABLE editing_tasks (id, project_id, title, brief, ...);
CREATE TABLE conversations (id, project_id, editing_task_id, title, ...);  -- schema v7 新增
CREATE TABLE messages (id, conversation_id, ...);

-- 素材归项目
CREATE TABLE assets (id, project_id, kind, ...);

-- storyboard 归剪辑任务
CREATE TABLE storyboard_versions (
  id, project_id, editing_task_id, version_number, ...  -- schema v7 新增 editing_task_id
);

-- timeline 通过 storyboard 间接归剪辑任务
CREATE TABLE timeline_versions (
  id, project_id, storyboard_version_id, version_number, ...
);
```

查询证据：
- `taskrouter.rs:704`: `SELECT ... FROM storyboard_versions WHERE project_id = ?1 AND editing_task_id = ?2`
- `taskrouter.rs:712`: `SELECT timeline.* FROM timeline_versions timeline JOIN storyboard_versions storyboard ON ... WHERE timeline.project_id = ?1 AND storyboard.editing_task_id = ?2`
- `timeline.rs:1030`: 同样通过 JOIN `storyboard_versions.editing_task_id` 筛选 timeline
- `agentloop.rs:1618/2810`: 检查 storyboard 时同时验证 `project_id` 和 `editing_task_id`

**事实**：
- storyboard 直接属于 `editing_task`（通过 `editing_task_id` 外键）
- timeline 间接属于 `editing_task`（通过 `storyboard_version_id` JOIN）
- preview 基于 timeline，因此也间接属于 `editing_task`
- Jianying draft 基于 timeline，同样间接属于 `editing_task`
- **conversation 只是同一个 editing_task 内的对话容器**，可以有多个会话讨论同一个剪辑任务，但产物不归会话所有

## Phase 2: 文档与代码的不一致

`docs/architecture.md:54` 写道：
> "会话、storyboard、timeline 和 preview 均被限制在该任务内"

这句话**正确但模糊**——"限制在任务内"可以被误读为"归会话所有"。

实际架构是：
```
Project (项目)
└── Assets (素材，项目级复用)
└── Editing Task (剪辑任务，创作目标)
    ├── Conversation 1, 2, ... (会话，对话容器)
    │   └── Messages (消息)
    ├── Storyboard v1, v2, ... (故事板版本)
    ├── Timeline v1, v2, ... (时间线版本，JOIN storyboard)
    ├── Preview (基于 timeline)
    └── Jianying Draft (基于 timeline)
```

**会话是平行的对话通道，产物属于剪辑任务本身**。

当前文档未明确：
1. 一个 editing_task 可以有多个 conversation（例如第一轮讨论失败，开新会话继续同一任务）
2. 产物查询时只需 `(project_id, editing_task_id)`，不需要 `conversation_id`
3. Task Router 负责把新消息路由到正确的 editing_task 或创建新任务

## Phase 3: 测试放置问题

当前状况：
- `timeline.rs:1071` 修复了 `select_timeline_candidate` 逻辑（核心 bugfix）
- 回归测试被加在 `agentloop.rs:3548–3557`（约9行压缩后）
- `agentloop.rs` 已达 3585 行，预算 3599 行，距离上限 14 行
- 使用 `#[rustfmt::skip]` 压缩测试格式以绕过预算

**为什么错**：
- `select_timeline_candidate` 是 `timeline.rs` 的公共函数
- 测试应该放在 `timeline.rs` 的 `#[cfg(test)]` 模块或独立的 `timeline_tests.rs`
- `agentloop.rs` 只应测试 agent loop 本身的路由、状态快照、技能编排——不应测试它依赖的领域函数
- 架构预算的目的是**强制拆分职责**，而不是让开发者用格式压缩绕过限制

参考已有模式：
- `preview.rs` 达到预算后，测试被提取为独立的 `preview_tests.rs`（通过 `#[path = "preview_tests.rs"] mod preview_tests;` 挂载）
- `timeline.rs` 已有内部 `#[cfg(test)] mod tests { ... }`，可以继续在那里添加

## Phase 4: 实施计划

### 4.1 迁移测试到正确位置

将 `agentloop.rs:3540–3557` 的 `delivery_tools_require_a_scoped_timeline_instead_of_creating_one` 测试中的多版本断言提取到 `timeline.rs` 的测试模块：

```rust
// timeline.rs 测试模块新增：
#[test]
fn select_timeline_candidate_picks_latest_when_multiple_versions_exist() {
    let v2 = TimelineVersion { 
        id: "v2".to_owned(), 
        version_number: 2, 
        project_id: "p1".to_owned(),
        storyboard_version_id: "sb1".to_owned(),
        clips: vec![],
        text_tracks: vec![],
        music_tracks: vec![],
        created_at: 2000,
    };
    let v1 = TimelineVersion { 
        id: "v1".to_owned(), 
        version_number: 1, 
        ..v2.clone()
    };
    
    // 候选按 version_number DESC 排列，首条是最新版
    assert_eq!(
        select_timeline_candidate(&[v2.clone(), v1], None, None)
            .map(|t| t.id),
        Some("v2".to_owned())
    );
}
```

`agentloop.rs` 的测试保留原有的"工具必须有作用域"逻辑检查，删除多版本数值断言（那是 `timeline.rs` 的单元测试职责）。

### 4.2 澄清架构文档

在 `docs/architecture.md` 的"数据所有权与安全"或"当前实现细节"章节补充：

```markdown
### 作用域架构

```
Project (项目)
├── Assets (素材，项目级复用)
└── Editing Tasks (剪辑任务，创作目标单元)
    ├── Conversations (会话，对话容器；一个任务可有多个会话)
    │   └── Messages
    ├── Storyboard Versions (故事板，归任务)
    ├── Timeline Versions (时间线，通过 storyboard 归任务)
    └── Previews / Jianying Drafts (基于 timeline，归任务)
```

**会话（conversation）只是对话容器**，不拥有产物。用户可在同一剪辑任务下开启多个会话（例如第一轮讨论后重新开始），所有会话共享该任务的 storyboard、timeline 和 preview 版本。

产物查询和创建只需 `(project_id, editing_task_id)`，不依赖 `conversation_id`。Task Router 负责把新消息路由到正确的任务或创建新任务；Conversation Router 决定是直接回复还是启动 Agent run。
```

### 4.3 更新 `.harness/architecture-budgets.json`

`agentloop.rs` 测试迁移后行数会降到约 3548 行（删除 9 行多版本断言），远低于预算 3599。

`timeline.rs` 当前 1848 行，预算 1848 行；新增 20 行测试会到 1868 行，需要更新预算到 1868。**但这次是合理的增长**——是把错误放置的测试迁移回正确的模块，而不是膨胀。

同时在预算 JSON 添加注释或在 commit message 说明：
> `timeline.rs` 预算从 1848 → 1868 (+20) 是迁移 `select_timeline_candidate` 的回归测试从 `agentloop.rs` 回归正确模块，符合解耦原则。

### 4.4 补充 AGENTS.md 解耦指引

在 `src-tauri/src/AGENTS.md` 增加测试放置规则：

```markdown
## 测试放置与架构预算

- 单元测试必须放在被测试函数所在的模块（`#[cfg(test)] mod tests`）或同名 `_tests.rs` 文件
- 集成测试放在 `tests/` 目录
- 架构预算（`.harness/architecture-budgets.json`）的行数/字符限制不得用格式压缩（`#[rustfmt::skip]`、单行展开等）绕过；超限时应拆分模块职责，而不是压缩格式
- 已有 `preview.rs` → `preview_tests.rs` 提取模式可作为参考
- 预算放宽需要在 commit message 说明合理性（例如：迁移错误放置的测试、补充缺失的回归覆盖等）
```

## Verification

1. 将 `agentloop.rs` 的多版本断言迁移到 `timeline.rs` 测试模块
2. `cargo test --lib` — 新测试通过，`agentloop.rs` 既有测试仍通过
3. `cargo fmt --check` — 新测试使用标准格式，不压缩
4. 更新 `docs/architecture.md` 补充作用域架构图
5. 更新 `.harness/architecture-budgets.json`：
   - `agentloop.rs`: 保持 3599（实际降到 ~3548）
   - `timeline.rs`: 1848 → 1868
6. `npm run harness:check` — 预算通过，ratchet 接受合理增长
7. 补充 `src-tauri/src/AGENTS.md` 测试放置规则
8. 创建变更记录 `docs/changes/2026-08-16-clarify-scope-architecture-and-decouple-tests.md`
9. 更新 `TASKS.md` 当前窗口

## Trade-offs

- **不做**：把 `timeline.rs` 也强行保持在 1848 行。那会重复刚才的错误——用压缩或错误放置绕过预算，而不是承认合理的职责边界
- **接受**：`timeline.rs` 预算从 1848 → 1868 (+20)，因为这是修正错误测试放置的必要代价
- **后续**：如果 `timeline.rs` 继续增长到 2000+，那时再考虑提取独立的 `timeline_tests.rs`（仿照 `preview_tests.rs`）

## Files to modify

1. `src-tauri/src/timeline.rs` — 新增 `select_timeline_candidate_picks_latest_when_multiple_versions_exist` 测试
2. `src-tauri/src/agentloop.rs` — 删除测试中的多版本断言，保留作用域校验逻辑
3. `.harness/architecture-budgets.json` — 更新 `timeline.rs` 预算 1848 → 1868
4. `docs/architecture.md` — 补充作用域架构图与会话/产物关系说明
5. `src-tauri/src/AGENTS.md` — 补充测试放置与预算绕过禁止规则
6. `docs/changes/2026-08-16-clarify-scope-architecture-and-decouple-tests.md` — 变更记录
7. `TASKS.md` — 当前窗口记录
