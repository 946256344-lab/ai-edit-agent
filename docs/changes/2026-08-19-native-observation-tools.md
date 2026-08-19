# 2026-08-19：NativeToolLoop 迁移剩余只读观察工具

NativeToolLoop 的原生工具目录现在覆盖全部 9 个只读观察技能：

- `get_edit_status`
- `get_asset_health_summary`
- `list_assets`
- `search_assets`
- `search_asset_segments`
- `search_music`
- `get_storyboard`
- `get_timeline`
- `get_text_capabilities`

每项定义集中在 `src-tauri/src/agentloop/tools.rs`，使用 `strict: true`、`additionalProperties: false` 和完整 `required`。搜索文本、枚举、时长、评分和分页参数在 schema 与 Native Rust 边界双重限制；语义可选值使用 nullable。工具定义不包含项目、会话或本地路径作用域参数。

执行仍统一复用现有 `apply_skill`。Native loop 只接受带匹配 `tool/status` 的成功结果，并递归移除作用域和本地路径字段后再写入 `function_call_output`；技能错误继续转换为脱敏安全错误。编辑、下载、分析、预览和交付工具不会因本次迁移进入默认原生目录，`render_preview` 仍由现有请求策略单独控制。

固定 Rust 测试覆盖工具唯一性、只读目录选择、严格 schema、搜索参数边界、安全结果包络和每个新增工具的 function call 选择。

同步文档：

- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `README.md`
- `TASKS.md`
