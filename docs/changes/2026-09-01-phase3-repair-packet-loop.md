# 2026-09-01: Phase 3 结构化修复包回传循环

## 背景

多次用户报告 Phase 3 连续三轮失败。根因是旧实现里 `enforce_phase3_scope` **一旦发现第一个错误就返回**，
模型每轮只能看到一个模糊的文本错误（如 "must keep every covered beat in the selected order"），
既不知道错在哪个镜头，也不知道允许怎么改。而 `agnes-2.5-flash` 这类模型在同时处理
选镜、拆镜、排序、时长、字幕、JSON schema 时本来就容易顾此失彼，一次只反馈一个问题
无法形成有效收敛。

## 方案：结构化 RepairPacket，模型保持全局主创，Rust 只做机械兜底

### 新模块 `storyboard/repair.rs`

- `StoryboardIssue`：`kind`（机器可读）、`message`（人类可读）、`affectedShots`（镜头号）、
  `needsModelDecision`（是否需要模型决策）、`allowedChanges`（允许的修复方向——是约束
  边界不是操作手册，模型仍自主决定具体做法）。
- `RepairRecord`：修复记忆条目（round / kind / shots / resolved），用于告诉模型
  前几轮修过什么、哪些已解决，避免回退。
- `ShotSnapshot`：上一轮候选的精简快照（序号/beat/asset/时长/源范围）。模型每次
  Phase 3 是独立请求、没有对话上下文，快照让它能"看到自己上一版输出"并接着改。
- `RepairPacket`：一轮候选的全部问题 + `previousShots` + `frozenShots` + `repairHistory`。
- `frozen_shot_indices`：未被任何问题点名的镜头视为已确认正确（局部冻结）。
- `repair_packet_prompt_block`：把修复包序列化成 JSON 注入下一轮 Phase 3 prompt，
  措辞为「你是编辑，只修被点名的镜头，未列出的镜头保持不变；规则是边界不是微指令」。

### `storyboard/phases.rs`

- `phase3_fine_edit` 返回 `(candidate, issues)`：请求失败或 JSON 无法解析才是 `Err`；
  可解析的候选即使有结构性问题也返回，问题交由调用方决定是否回传模型。
- `enforce_phase3_scope` 重写为 `collect_phase3_issues`：
  - **收集全部问题**（beat 乱序、越出候选池、beat 内重复素材、首镜头被换、给
    uncovered beat 补镜头），而不是第一个错误就返回；
  - 每个问题带 `allowedChanges`，模型能据此做决策；
  - 无歧义的子镜头字段标准化（`beatPartIndex`/`beatPartCount`/`splitRole`）仍无条件执行。
- 测试 `collect_phase3_issues_reports_every_violation_not_just_the_first` 证明多问题同报。

### `storyboard.rs` 重试循环

- `issues` 为空或全部 `needs_model_decision=false` → 接受候选并走 `normalize` 机械兜底；
- 存在语义问题 → 构造带完整上下文的 `RepairPacket`：
  - `frozenShots`：未被问题点名的镜头（局部冻结，模型保持不动）；
  - `previousShots`：上一轮候选快照（模型据此接着改）；
  - `repairHistory`：前几轮修过什么、是否已解决；
- 模型请求失败（网络/超时）→ 打包为 `needs_model_decision=false` 的 issue，直接重试；
- 终态失败信息归一为修复包第一条 issue 的 message，并记录
  `needs_model_decision`（区分"模型修不好"与"纯请求失败"）。

## 决策

- 拆分语义（换哪个素材、拆几段）永远是模型的决策；Rust 绝不替模型改素材。
- 一个问题产生一个 issue，多个问题同轮反馈，避免逐轮挤牙膏。
- 机械问题（请求失败重试、字段补齐）不进模型，节省修复轮次。
- 已确认正确的镜头冻结不动、修复记忆防回退、候选快照让独立请求"接着改"：
  这是 agent repair loop（观察→反馈→修复→再观察）而非一次性补丁器。
- `allowedChanges` 是约束边界不是操作手册：prompt 明确"你是编辑，规则不是微指令"，
  避免限制模型发挥。

## 变更范围

- `src-tauri/src/storyboard/repair.rs`（新模块 + 测试）
- `src-tauri/src/storyboard/phases.rs`（`phase3_fine_edit` 返回 issues、`collect_phase3_issues` + 测试）
- `src-tauri/src/storyboard.rs`（重试循环 + 冻结/快照/修复记忆 + 终态错误归一）
- `docs/api.md`（Phase 3 契约 + 维护记录）
- 本记录

## 验证

- `cargo test --manifest-path src-tauri/Cargo.toml storyboard::`：54 passed
- `cargo test --manifest-path src-tauri/Cargo.toml`：268 passed
- `cargo build`：无 error / warning
- `npm run lint`、`npm run harness:check`：通过