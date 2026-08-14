# 项目事实问答证据门

## 变更

- Conversation Router 的 question 响应增加 `informationScope=general|project`；顶层 route 保持 `respond/clarify/run`。
- `project + respond` 被协议校验拒绝，项目事实问题必须进入 Agent loop，由模型选择观察工具。
- 非法的 `project + respond` 在同一预算内只纠正一次；仍无效时返回固定诚实回复，不把协议错误或模型猜测交给用户。项目事实 run 全程只读且至少需要一次成功观察。
- 新增只读 `get_asset_health_summary`，返回项目级健康计数、扫描状态、最近检查时间和安全原因码计数。
- schema v14 为 `asset_source_health` 增加 `reason_code`；扫描将 Windows 文件元数据错误映射为固定脱敏原因，不保存或返回路径与原始错误。

## 边界

- 不新增关键词路由、独立分类模型或第四种顶层 route。
- 既有健康记录不会臆测补写原因；重新扫描前 `reasonEvidenceAvailable` 可能为 false，此时模型必须说明具体原因尚不确定。
- 健康观察是只读工具，不主动访问源文件，也不产生编辑副作用。
- 原因覆盖率以 `reasonedFailureCount`/`unexplainedFailureCount` 明示；只在所有失败都有安全原因码时声明原因证据完整。

## 验证

- Rust library check 和 107 个既有单元测试在初次实现后通过；新增路由、健康汇总和工具契约测试随最终验证执行。
