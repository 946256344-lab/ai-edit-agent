# 项目与剪辑会话编辑

项目切换列表和剪辑会话列表统一增加操作菜单。项目支持重命名、打开项目设置和确认删除；会话支持重命名和复用现有确认删除。点击侧栏空白处会收起已展开的项目列表和操作菜单。

新增 `rename_project`、`rename_editing_session` 和 `delete_project` 三个 Tauri 命令。会话重命名同步更新 editing task 与所属 conversations，避免刷新后恢复旧标题。项目删除清理项目内素材索引、派生分析文件、会话、Agent 记录、storyboard、时间线和本地预览缓存；不删除原始媒体与已经创建的外部剪映草稿。

同步文档：`docs/api.md`。

前端 lint/build、Rust fmt/check、分支、架构预算和真实 Tauri WebView 烟雾检查通过，桌面开发版无运行时错误或页面横向溢出。新增 Rust 回归覆盖名称同步、项目级联删除和其他项目不受影响；完整 Rust 测试 354 项中 353 项通过，原有 `subtitle_newsbar` 配方校验失败。本分支未修改的 `outbound_http.rs` 仍触发现有 Agent 契约检查，`test-doc-sync` 仍有原有断言失败。
