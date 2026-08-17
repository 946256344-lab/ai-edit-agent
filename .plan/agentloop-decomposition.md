# agentloop.rs 拆分计划

## 目标

将 `src-tauri/src/agentloop.rs`（3684 行）拆分为 4 个子模块 + 1 个精简核心：

1. **agentloop/skills.rs** - 技能执行与参数校验（~800 行）
2. **agentloop/prompt.rs** - 提示构建与历史渲染（~600 行）
3. **agentloop/runtime.rs** - 有界循环与步骤管理（~500 行）
4. **agentloop/schema.rs** - 决策 schema 与解析（~400 行）
5. **agentloop.rs** 核心 - 保留 ~1400 行

## 当前文件结构分析

### 第一部分：imports 与模块声明（1-37 行）
- 外部依赖导入
- `mod policy;` 声明
- 从 policy 重导出的类型

### 第二部分：常量与 schema（39-419 行）
- 常量：`MAX_STEPS=10`, `AGENT_STEP_TIMEOUT=120s`, `AGENT_RUN_TIMEOUT=300s`
- **路由决策 schema**（53-88 行）：
  - `ConversationRouteDecision` enum
  - `InitialAgentSkill` struct
  - `ConversationRouteResponse` struct
- **路由函数**（90-294 行）：
  - `decide_conversation_route`
  - `try_build_route_decision`
  - `clarification_resolution`
  - `question_scope_allows_route`
- **Agent schema**（303-419 行）：
  - `AgentStep` struct
  - `AgentStateSnapshot` + 嵌套子结构（scope/assets/artifacts/steps）
  - `LoopState<'a>` struct
  - `AgentLoopResult`、`AgentLoopTerminalStatus`

### 第三部分：核心运行时（433-679 行）
- `run_agent_loop` - 主循环入口
- `run_agent_loop_with_initial_skill` - 带初始技能的循环
- 循环状态管理和终止条件判断

### 第四部分：步骤执行（681-1454 行）
- `first_model_step` helper
- `run_explicit_command` - 显式命令直通路径
- `AgentLoopControl` enum（773-781 行）
- `reject_user_restricted_tool` - 工具黑名单检查
- `execute_initial_skill` - 初始技能执行
- **`run_step`** - 单步模型决策与技能执行（925-1454 行）
  - 构建 prompt
  - 调用模型
  - 解析决策
  - 执行技能
  - 处理错误

### 第五部分：结果处理与辅助函数（1456-1550 行）
- `finalize_result`、`finalize_result_helper`、`finalize_terminal`
- `result_has_artifact`
- `step_args` - 从模型响应提取参数
- `remaining_model_timeout`

### 第六部分：历史与状态加载（1552-1677 行）
- **历史管理**：
  - `MAX_HISTORY_MESSAGES=12`, `MAX_HISTORY_CHARS=8000`
  - `load_message_history` - 从 DB 加载会话历史
  - `render_history` - 渲染为文本
- `load_pending_clarification` - 加载待澄清状态
- **状态快照构建**：
  - `load_asset_availability` - 素材可用性
  - `current_artifact_presence` - 产物存在性（含 storyboard/timeline/preview/jianying）
  - `preview_presence`、`jianying_presence` helpers

### 第七部分：前置条件与提示（1843-2267 行）
- `unmet_conditions` - 未满足条件诊断
- `build_agent_state_snapshot` - 构建完整状态快照
- `deterministic_prerequisite_hints` - 确定性前置条件提示
- `produced_artifact_for_tool`、`persisted_artifact_for_tool` - 工具产物映射
- `safe_step_error_code`、`diagnostic_count` - 错误码提取
- `safe_tool_failure_context` - 安全失败诊断
- `safe_failure_explanation` - 失败解释校验
- `should_redirect_storyboard_after_failed_generation`
- `project_fact_completion_instruction`
- **`build_step_prompt`** - 构建完整的模型 prompt（2176-2267 行）

### 第八部分：技能执行（2269-2815 行）
- **`apply_skill`** - 巨型 match 分发到各技能实现（2271-2815 行）
  - `get_edit_status`
  - `search_music`、`download_music`、`use_online_music`
  - `list_assets`、`get_asset_health_summary`、`search_assets`、`search_asset_segments`
  - `request_asset_analysis`
  - `get_storyboard`、`get_timeline`、`get_text_capabilities`
  - `generate_storyboard`
  - `create_timeline_draft`
  - `replace_clips`、`change_clip_duration`、`reorder_clips`
  - `replace_text_tracks`、`replace_music_tracks`
  - `render_preview`、`create_jianying_draft`

### 第九部分：状态读取与辅助（2817-2973 行）
- `EditArtifactState` struct
- `artifact_status_message`、`edit_status_message` - 状态消息生成
- `read_scoped_edit_status` - 读取作用域编辑状态
- `select_timeline_for_tool` - 为工具选择 timeline
- `build_timeline_snapshot` - 构建 timeline 快照
- `upsert`、`upsert_timeline` - timeline 列表更新

### 第十部分：测试（2974-3684 行）
- 约 700 行单元测试
- 覆盖路由决策、工具策略、状态快照、前置条件等

## 拆分方案

### 1. agentloop/schema.rs（~400 行）

**职责**：决策 schema、路由决策与解析

**包含内容**：
- `ConversationRouteDecision` enum
- `InitialAgentSkill` struct
- `ConversationRouteResponse` struct
- `AgentStep` struct
- `AgentStateSnapshot` 及其嵌套子结构（scope/assets/artifacts/steps）
- `AgentLoopResult`、`AgentLoopTerminalStatus`
- 路由函数：
  - `decide_conversation_route`
  - `try_build_route_decision`
  - `clarification_resolution`
  - `question_scope_allows_route`
- schema 解析辅助：
  - `step_args` - 提取参数
  - `parse_declared_goal`（从 policy.rs 引用）

**导出**：
```rust
pub(crate) use schema::{
    ConversationRouteDecision, InitialAgentSkill, ConversationRouteResponse,
    AgentStep, AgentStateSnapshot, AgentScopeSnapshot, 
    AssetAvailabilitySnapshot, ArtifactPresenceSnapshot,
    VersionArtifactSnapshot, TimelineArtifactSnapshot, JianyingArtifactSnapshot,
    ExecutedStepSummary, PendingClarificationSnapshot,
    AgentLoopResult, AgentLoopTerminalStatus,
    decide_conversation_route, step_args,
};
```

**测试迁移**：
- `step_args_removes_meta_keys`
- `step_args_survives_non_object_decisions`
- 路由决策相关测试

---

### 2. agentloop/prompt.rs（~600 行）

**职责**：提示构建、历史渲染、状态快照构建

**包含内容**：
- 常量：`MAX_HISTORY_MESSAGES=12`, `MAX_HISTORY_CHARS=8000`
- **历史管理**：
  - `load_message_history` - 从 DB 加载
  - `render_history` - 渲染为文本
- **状态加载**：
  - `load_pending_clarification`
  - `load_asset_availability`
  - `current_artifact_presence`
  - `preview_presence`
  - `jianying_presence`
- **前置条件与提示**：
  - `unmet_conditions`
  - `build_agent_state_snapshot`
  - `deterministic_prerequisite_hints`
  - `project_fact_completion_instruction`
- **核心 prompt 构建**：
  - `build_step_prompt` - 构建完整的模型 prompt

**导出**：
```rust
pub(crate) use prompt::{
    load_message_history,
    render_history,
    load_pending_clarification,
    build_agent_state_snapshot,
    deterministic_prerequisite_hints,
    build_step_prompt,
};
```

**测试迁移**：
- 状态快照相关测试（如果有）
- 历史渲染测试（如果有）

---

### 3. agentloop/skills.rs（~800 行）

**职责**：技能执行、参数校验、产物映射

**包含内容**：
- **工具产物映射**：
  - `produced_artifact_for_tool`
  - `persisted_artifact_for_tool`
- **错误处理**：
  - `safe_step_error_code`
  - `diagnostic_count`
  - `safe_tool_failure_context`
  - `safe_failure_explanation`
  - `should_redirect_storyboard_after_failed_generation`
- **核心技能分发**：
  - `apply_skill` - 巨型 match（2271-2815 行，约 545 行）
- **辅助函数**：
  - `select_timeline_for_tool`
  - `build_timeline_snapshot`
  - `upsert`、`upsert_timeline`
- **状态读取**：
  - `EditArtifactState` struct
  - `artifact_status_message`
  - `edit_status_message`
  - `read_scoped_edit_status`

**导出**：
```rust
pub(crate) use skills::{
    apply_skill,
    read_scoped_edit_status,
    safe_step_error_code,
    safe_tool_failure_context,
};
```

**测试迁移**：
- `storyboard_generation_failure_redirects_without_reasking_for_a_brief`
- 状态消息相关测试（如果有）

---

### 4. agentloop/runtime.rs（~500 行）

**职责**：有界循环、步骤管理、控制流

**包含内容**：
- 常量：`MAX_STEPS=10`, `AGENT_STEP_TIMEOUT`, `AGENT_RUN_TIMEOUT`
- `LoopState<'a>` struct（移到这里或保留在 schema.rs）
- `AgentLoopControl` enum
- **核心循环**：
  - `run_agent_loop`
  - `run_agent_loop_with_initial_skill`
  - `first_model_step`
- **步骤执行**：
  - `run_step` - 单步决策与执行（925-1454 行，约 530 行）
  - `execute_initial_skill`
  - `reject_user_restricted_tool`
- **显式命令**：
  - `run_explicit_command`
- **结果处理**：
  - `finalize_result`
  - `finalize_result_helper`
  - `finalize_terminal`
  - `result_has_artifact`
  - `remaining_model_timeout`

**导出**：
```rust
pub(crate) use runtime::{
    LoopState,
    run_agent_loop,
    run_agent_loop_with_initial_skill,
    run_explicit_command,
};
```

**测试迁移**：
- `finalize_result_keeps_the_last_concrete_outcome`
- 循环控制相关测试（如果有）

---

### 5. agentloop.rs 核心（~1400 行）

**保留内容**：
- imports（简化，只保留必需的外部依赖）
- `mod policy;` 和 policy 重导出
- **新增子模块声明**：
  ```rust
  mod schema;
  mod prompt;
  mod skills;
  mod runtime;
  ```
- **重导出公共 API**：
  ```rust
  pub(crate) use schema::{
      ConversationRouteDecision, InitialAgentSkill, ConversationRouteResponse,
      AgentStateSnapshot, AgentLoopResult, AgentLoopTerminalStatus,
      decide_conversation_route,
  };
  pub(crate) use runtime::{
      run_agent_loop, run_agent_loop_with_initial_skill, run_explicit_command,
  };
  pub(crate) use skills::read_scoped_edit_status;
  ```
- **测试模块**（~700 行）：
  - 保留在核心文件，通过 `use super::*;` 和子模块路径访问所有函数
  - 或者按模块拆分到各子模块的 `#[cfg(test)] mod tests`

**行数估算**：
- imports + 模块声明：~50 行
- 重导出：~30 行
- 测试：~700 行
- 其他可能保留的小型辅助：~100 行
- **合计**：~880 行（留有余地到 1400 行）

---

## 拆分步骤

### Phase 1: 创建 schema.rs（最独立）
1. 创建 `src-tauri/src/agentloop/schema.rs`
2. 移动所有 schema 定义和路由决策函数
3. 添加必要的 imports 和 `pub(crate)` 可见性
4. 在 `agentloop.rs` 添加 `mod schema;` 和重导出
5. 更新相关测试导入路径

### Phase 2: 创建 prompt.rs（依赖 schema）
1. 创建 `src-tauri/src/agentloop/prompt.rs`
2. 移动历史管理、状态加载和 prompt 构建函数
3. 引入 `use super::schema::*;` 获取 schema 类型
4. 在 `agentloop.rs` 添加 `mod prompt;` 和重导出

### Phase 3: 创建 skills.rs（依赖 schema + prompt）
1. 创建 `src-tauri/src/agentloop/skills.rs`
2. 移动 `apply_skill` 和所有技能辅助函数
3. 引入 schema 和 runtime 类型（`LoopState`）
4. 在 `agentloop.rs` 添加 `mod skills;` 和重导出

### Phase 4: 创建 runtime.rs（依赖前三个模块）
1. 创建 `src-tauri/src/agentloop/runtime.rs`
2. 移动核心循环和步骤执行函数
3. 引入 schema、prompt、skills 模块
4. 在 `agentloop.rs` 添加 `mod runtime;` 和重导出

### Phase 5: 清理核心 agentloop.rs
1. 删除已移动的代码
2. 保留必要的重导出和测试
3. 确保所有公开 API 路径不变

### Phase 6: 测试迁移（可选优化）
1. 考虑将测试按模块拆分到各子模块
2. 或保留在核心文件，通过模块路径访问

---

## 完成门

1. **编译通过**：`cargo check --manifest-path src-tauri/Cargo.toml`
2. **格式检查**：`cargo fmt --manifest-path src-tauri/Cargo.toml --check`
3. **测试通过**：`cargo test --manifest-path src-tauri/Cargo.toml`
4. **架构预算**：
   - `agentloop.rs` ≤ 1500 行
   - `agentloop/schema.rs` ≤ 500 行
   - `agentloop/prompt.rs` ≤ 700 行
   - `agentloop/skills.rs` ≤ 900 行
   - `agentloop/runtime.rs` ≤ 600 行
5. **公开契约不变**：
   - `lib.rs` 中的 Tauri 命令路径保持或更新为子模块路径
   - 其他模块对 agentloop 的导入路径保持兼容
6. **harness 检查**：`npm run harness:check`
7. **diff 检查**：`git diff --check`

---

## 风险与注意事项

1. **生命周期标注**：`LoopState<'a>` 的生命周期必须正确传播
2. **循环依赖**：确保模块依赖是单向的（schema → prompt → skills → runtime）
3. **测试覆盖**：拆分后确保所有测试仍能访问私有函数（通过 `pub(crate)` 或 `#[cfg(test)]` 辅助）
4. **错误传播**：所有 `Result<T, String>` 返回值正确传播
5. **policy 模块依赖**：现有 `mod policy;` 及其重导出不变，各子模块通过 `use super::policy::*;` 访问

---

## 参考：已完成的 assets 拆分

assets.rs 拆分为：
- `assets.rs` 核心（793 行）
- `assets/analysis.rs`（1036 行）
- `assets/visual.rs`（1032 行）
- `assets/health.rs`（339 行）
- `assets/library.rs`（833 行）

类似模式：
1. 核心文件保留模块声明和重导出
2. 子模块独立实现各自职责
3. 通过 `pub(crate)` 保持内部可见性
4. Tauri 命令注册路径从 `assets::*` 更新为 `assets::submodule::*`

本次 agentloop 拆分遵循相同模式，确保：
- 职责清晰分离
- 模块间依赖单向
- 公开 API 稳定
- 测试完整覆盖
