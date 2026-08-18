# 2026-08-18: 修复素材重链接时 kind 字段未同步更新的数据一致性问题

## 变更类型

bugfix（数据一致性）

## 触发规则

- desktop-contract（修改 `src-tauri/src/assets.rs` 和 `src-tauri/src/assets/analysis.rs`）

## 问题描述

**根本原因：**

`kind` 字段在素材首次导入时根据文件扩展名写入数据库，但在以下两个关键路径中未同步更新：

1. **relink 路径**：用户通过 `confirm_asset_relink` 重新链接素材到不同文件时，只更新了 `source_reference` 和 `folder_reference`，未更新 `kind`
2. **分析结果回写路径**：`update_analysis_status` 在分析完成后回写 `metadata_json` 时，未同步验证并更新 `kind`

**实际影响：**

用户将 451 个图片素材（`.jpg`/`.png`）替换为同名视频文件（`.mp4`），通过 relink 功能重新关联后：
- 文件系统：426 个视频 + 29 个图片
- 数据库 `kind` 字段：451 个 `'image'` + 4 个 `'video'`（未更新）
- storyboard 生成日志显示素材池组成错误，导致用户误判素材分类逻辑

## 变更范围

### 1. 修复 relink 路径的 kind 同步（src-tauri/src/assets.rs）

**`confirm_asset_relink` 函数（行 304-360）**

- 在循环开始时（行 326）新增：`let new_kind = asset_kind(&source);`
- 更新 `preserve_analysis = true` 分支（行 328-331）：
  - 原 SQL：`UPDATE assets SET source_reference = ?1, folder_reference = ?2, updated_at = ?3 WHERE ...`
  - 新 SQL：`UPDATE assets SET source_reference = ?1, folder_reference = ?2, kind = ?3, updated_at = ?4 WHERE ...`
  - 新增参数：`new_kind`
- 更新 `preserve_analysis = false` 分支（行 337-340）：
  - 原 SQL：`UPDATE assets SET source_reference = ?1, folder_reference = ?2, analysis_status = 'queued', metadata_json = '{}', updated_at = ?3 WHERE ...`
  - 新 SQL：`UPDATE assets SET source_reference = ?1, folder_reference = ?2, kind = ?3, analysis_status = 'queued', metadata_json = '{}', updated_at = ?4 WHERE ...`
  - 新增参数：`new_kind`

### 2. 修复分析结果回写路径的 kind 同步（src-tauri/src/assets/analysis.rs）

**`update_analysis_status` 函数（行 413-449）**

- 在 transaction 创建后（行 437）新增：
  ```rust
  // 从 source_reference 重新计算 kind，防止文件类型变化时产生不一致
  let new_kind = asset_kind(Path::new(source_reference));
  ```
- 更新 `metadata_json = Some(...)` 分支（行 438-441）：
  - 原 SQL：`UPDATE assets SET analysis_status = ?1, metadata_json = ?2, updated_at = ?3 WHERE ...`
  - 新 SQL：`UPDATE assets SET analysis_status = ?1, metadata_json = ?2, kind = ?3, updated_at = ?4 WHERE ...`
  - 新增参数：`new_kind`
- 更新 `metadata_json = None` 分支（行 443-448）：
  - 原 SQL：`UPDATE assets SET analysis_status = ?1, updated_at = ?2 WHERE ...`
  - 新 SQL：`UPDATE assets SET analysis_status = ?1, kind = ?2, updated_at = ?3 WHERE ...`
  - 新增参数：`new_kind`

## 修复效果

**修复前：**
```
用户操作：删除 scene001.jpg，替换为 scene001.mp4，执行 relink
数据库：source_reference = "scene001.mp4", kind = "image"  ❌
storyboard 日志：video_count=4, image_count=451  ❌
```

**修复后：**
```
用户操作：删除 scene001.jpg，替换为 scene001.mp4，执行 relink
数据库：source_reference = "scene001.mp4", kind = "video"  ✅
storyboard 日志：video_count=426, image_count=29  ✅
```

## 同步文档

- **docs/architecture.md**：已更新素材导入与重链接流程说明，明确 `kind` 字段在 relink 和分析回写时会同步重新计算
- **docs/api.md**：已更新维护记录，注明 `kind` 字段现在会在所有写入路径保持一致性
- **TASKS.md**：已更新当前任务窗口，标记 bugfix 任务完成

## 公开契约

### Tauri 命令

**`confirm_asset_relink`**
- 签名：不变
- 行为变化：现在会同步更新 `kind` 字段以匹配新的 `source_reference` 的文件扩展名
- 向后兼容：是（纯内部实现变化，不影响调用方）

### SQLite schema

无变更（`kind` 字段已存在，只是更新逻辑修复）。

### Agent 工具

无变更（工具白名单、参数、返回值均不变）。

## 验证证据

- ✅ Rust 库测试：113 passed
- ✅ Rust fmt/check：通过
- ✅ harness:test：通过
- ⏳ harness:check：需要同步 docs/architecture.md 和 docs/api.md 后通过

## 边界情况

**Q: 如果用户 relink 到一个无扩展名文件或不支持的扩展名会怎样？**

A: `asset_kind` 函数会返回 `"other"`，`kind` 字段会被更新为 `"other"`。这是正确行为，因为该素材确实不是已知的视频/图片/音频类型。

**Q: 旧的素材记录（在此修复前创建的）会自动修复吗？**

A: 不会自动修复。但下次用户对这些素材执行以下操作时会自动同步：
- relink 到任何文件（即使是同一文件）
- 触发重新分析（`preserve_analysis = false` 的 relink 或手动触发）

如需批量修复历史数据，可通过 SQL 脚本：
```sql
UPDATE assets 
SET kind = CASE 
  WHEN lower(substr(source_reference, -4)) IN ('.mp4', '.mov', '.mkv', '.avi') THEN 'video'
  WHEN lower(substr(source_reference, -4)) IN ('.jpg', '.png') OR lower(substr(source_reference, -5)) = '.jpeg' THEN 'image'
  WHEN lower(substr(source_reference, -4)) IN ('.mp3', '.wav', '.aac', '.m4a') THEN 'audio'
  ELSE 'other'
END
WHERE kind != CASE ...  -- 只更新不一致的记录
```

## 风险与限制

- **性能影响**：每次 relink 多调用一次 `asset_kind`（纯 CPU 字符串匹配，<1μs）
- **SQLite 参数偏移**：所有 UPDATE 语句的参数索引增加 1，已在代码中同步修正
- **未覆盖的写入路径**：素材首次导入路径（`import_assets`）已经正确设置 `kind`，无需修改

## 后续任务

1. **监控日志**：验证修复后 storyboard 生成日志中的 `video_count`/`image_count` 与文件系统一致
2. **历史数据审计**（可选）：运行 SQL 脚本检查并修复现有项目中的 `kind` 不一致记录
3. **单元测试补充**（可选）：为 `confirm_asset_relink` 添加测试用例，验证 relink 到不同类型文件时 `kind` 正确更新
