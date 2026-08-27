# Native 动态工具加载与日志读取

## 结果

- NativeToolLoop 每轮系统提示都提供完整、无状态标记的工具名称与一句话说明；首轮只注册 `load_tools`，模型从完整目录中选择 1–5 个业务工具，每次调用整体替换本轮加载集合。
- Provider 后续请求只携带常驻 loader 与当前已加载工具的完整 schema；加载结果从下一次请求生效，Rust 按该请求实际暴露集合逐调用复核，拒绝同一响应里尚未暴露的隐藏调用。
- 新增 `read_logs` 普通只读诊断能力，模型可在判断任务需要时自主加载；按 1-based 闭区间读取当前活动应用日志，省略范围时读取末尾最多 100 行，输出受 3500 字符和单行 500 字符预算并提供分页游标。用户明确禁止读取日志时执行门仍拒绝。
- 路径由 Rust 固定解析；模型不能选择文件或读取 Provider full trace。包含凭据、URL、UNC 或完整 Windows 路径的行整体遮蔽，普通错误、阶段信息、素材 ID 和诊断码保持可读，供模型判断后续修改。
- `load_tools` 和 `read_logs` 都不能满足项目事实观察门，不改变领域产物、SQLite schema 或公开 Tauri 命令。
- SQLite 历史不再按 12 条或 8000 字符裁剪。完整 Provider payload 使用 `o200k_base` 计量：超过 40K token 时调用同一 Provider 自主压缩旧历史，目标低于 30K，任何业务请求不得超过 60K。
- 压缩必须保留用户目标、明确约束、偏好、已作决定及原因、未解决问题；其他信息由模型自主取舍。当前请求、权威状态快照、近期原文和最新函数调用/结果对原样保留。

## 契约与测试

- Rust policy、TypeScript 工具名称镜像、运行步骤标签和 `agent_tool_contracts.v1.json` 已同步。
- 回归覆盖完整目录、strict loader schema、最多 5 个、集合替换、同响应隐藏调用拒绝、日志自主加载/明确禁止、固定路径、敏感行遮蔽、范围/分页、token 阈值、保护项及压缩失败封闭。

## 文档同步

- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/codebase/STRUCTURE.md`
- `TASKS.md`
