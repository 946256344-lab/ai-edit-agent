# Storyboard 信息点覆盖选镜

日期：2026-08-10

## 变更

- storyboard 生成请求改为先返回文案信息点，再为每个已覆盖信息点选择经过分析的本地素材时间范围。
- 每个镜头持久化 `beatId` 与 `matchLevel`；只允许 `direct` 和 `contextual`。
- 缺少可靠素材的内容保存为 `uncoveredBeatIds`，不会创建 `insufficient` 镜头或内部时间线片段。
- 后端拒绝跨镜头重叠复用同一视频源范围，防止同一段 B-roll 被用于多个信息点。
- 明确文案的 storyboard 非前置条件失败会回读给模型继续决策，不会重新要求用户描述成片目标。
- 对长英文文案增加最小阅读时长校验，避免将长脚本压成无法承载的短时间线。
- storyboard 界面显示直接/语境匹配与未覆盖信息点数量。

## 边界

本变更仅改善剪辑 Agent 的文案到真实素材选镜流程。没有新增配音、字幕、音乐、数据图形、最终导出或外部素材获取；未覆盖信息点必须保持诚实可见。

## 验证

- `cargo fmt --check`
- `cargo test --lib`
- `npm run lint`
- `npm run build`
