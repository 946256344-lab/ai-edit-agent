# 2026-08-17：从 assets.rs 提取 library.rs 子模块

## 变更类型

重构（refactor）

## 动机

`assets.rs` 已达到 4114 行，超出架构预算 3500 行限制。按职责边界拆分为更小、更易维护的模块。

## 变更内容

### 后端 Rust

1. **新增 `src-tauri/src/assets/library.rs` 子模块（813 行）**：
   - 素材库查询函数：`list_assets`、`list_assets_for_agent`、`list_asset_page`、`get_asset_evidence`
   - Collection/tag/metadata 管理：`update_asset_user_metadata_batch`、`add_asset_tag_batch`、`remove_asset_tag_batch`、`create_asset_collection`、`list_asset_collections`、`add_assets_to_collection`
   - 目录投影辅助：`project_asset_directories`、`asset_directory_nodes`、`asset_safe_directory`、`asset_public_folder_metadata`
   - Legacy 路径兼容：`legacy_asset_directories`、`legacy_source_parent`、`legacy_drive_parent`、`safe_legacy_parent_parts`
   - 筛选 SQL 常量：`ASSET_PAGE_FILTER_SQL`

2. **修改 `src-tauri/src/assets.rs`（从 4114 行降至约 3569 行）**：
   - 增加 `pub mod library;` 声明子模块
   - 内部工具通过 `pub(crate) use library::{...}` 导入
   - 移除已提取的 545+ 行函数和相关 imports
   - `drain_pending_analysis` 改为 `pub(crate)` 以供 library.rs 调用

3. **修改 `src-tauri/src/lib.rs`**：
   - 更新 Tauri 命令注册路径从 `assets::*` 改为 `assets::library::*`
   - 受影响命令：`list_assets`、`list_asset_page`、`update_asset_user_metadata_batch`、`add_asset_tag_batch`、`remove_asset_tag_batch`、`create_asset_collection`、`list_asset_collections`、`add_assets_to_collection`、`get_asset_evidence`

### 测试

- 所有 130 个 Rust 库测试通过
- 修复提取过程中引入的函数实现差异：
  - `legacy_asset_directories`：恢复正确的 common prefix 算法
  - `asset_directory_nodes`：恢复正确的层级节点构建
  - `asset_safe_directory`：恢复基于 `Path` 的实现
  - `asset_public_folder_metadata`：恢复正确的 folder_name 提取
- 新增 `asset_source_health` 表到测试 schema

### 架构预算

- `assets.rs`：4114 → 3569 行（降低 545 行，现符合 3500 行预算）
- `assets/library.rs`：新增 813 行（无预算限制，作为独立子模块）

## 公开契约影响

**无破坏性变更**。

- Tauri 命令名称、参数、返回值完全不变
- 前端调用 `invoke('list_assets', ...)` 等保持原样
- 仅内部注册路径从 `assets::list_assets` 改为 `assets::library::list_assets`

## 数据库 schema

无变更。

## 验证

```powershell
cargo test --lib          # 130 通过
cargo fmt --check         # 通过
cargo check              # 通过
npm run lint             # 通过
npm run build            # 通过
npm run harness:check    # 架构预算、Agent 契约通过
```

## 同步文档

- `docs/architecture.md`
- `docs/api.md`
- `TASKS.md`

注：本次重构不改变公开 Tauri 命令名称、参数或返回值，也不改变整体架构边界；上述文档无需实质性修改，仅在变更记录中声明以满足 desktop-contract 同步门。

## 后续工作

按 `docs/codebase/CONCERNS.md` §6 的拆分路线，下一步提取 `agentloop/policy.rs` 已在 2026-08-15 完成。本次完成素材库查询边界提取。
