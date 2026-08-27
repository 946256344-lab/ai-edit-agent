# Storyboard 本地语义召回与真实评分

## 目标

- Phase 2 每个 beat 向模型提供最多 12 个视频候选，模型查看关键帧后选择 1 个。
- 让“汽车”与“车辆”等没有词面重合的语义相关素材进入候选池。
- 让画面质量和素材新鲜度从固定值变为真实评分。
- 模型随 Windows 安装包分发，首次运行离线可用，失败时保留关键词降级。

## 实现

- `storyboard/phases.rs` 将候选上限从 5 调整为 12；每个 beat 编码 `requiredVisual + purpose` 后参与排序。
- `storyboard/scoring.rs` 将语义分范围从 0–50 调整为 0–30；有效向量使用余弦相似度，没有有效向量时继续使用英文词元和中文相邻双字。质量仍占 25 分、时长占 15 分、连续复用扣 10 分；新鲜度按 `10 / (1 + 使用任务数)` 计算。
- `assets/analysis.rs` 对统一为 320px 宽的关键帧计算拉普拉斯方差，归一化后取中位数。新分析直接写入质量分；已有技术就绪视频在首次 storyboard 前从本地关键帧补齐。
- 新鲜度只读取每个剪辑任务最新的 timeline，并在一个任务内按 `assetId` 去重，避免追加式历史版本和重复镜头虚增次数。
- `storyboard/semantic.rs` 从安装包资源加载 `BAAI/bge-small-zh-v1.5` 的 Xenova ONNX 转换，生成 512 维文本向量。素材文本只来自 subjects/actions/products/scene/OCR；向量、模型名、维度、版本和证据文本 SHA-256 保存在本地 `metadata_json`。
- 视觉 evidence 成功写入时增量生成向量；已有素材在首次 storyboard 前按项目批量补齐。数据库写入带旧 `metadata_json` 条件，避免覆盖并发视觉分析结果。
- `StoryboardSource.evidence_embedding` 禁止序列化，因此向量不会进入 Phase 3 Provider payload。模型缺失、完整性校验失败、推理失败、旧素材缺向量或维度不匹配时，Rust 使用原有词面排序。
- Phase 3 只接收 Phase 2 已选中的素材，不再读取全库；Rust 固定 Phase 1/2 的叙事结构并校验每个 beat 只能继续使用其 Phase 2 所选素材。向量和本机关键帧路径均禁止序列化给 Provider。
- 视觉 evidence 写入若发生并发元数据冲突，批次明确失败而不是误报完成；损坏的历史 timeline 只跳过新鲜度统计，不阻断 storyboard。

## 模型与安装包

- 使用 `fastembed 4.9.1` 和 `ort 2.0.0-rc.9`；未启用 fastembed 的 Hugging Face/online 特性。仓库虽仍声明 Rust 1.77.2，但现有锁文件及新增依赖树都含要求更高 Rust 版本的包，本次未把 1.77.2 兼容性作为已验证结论。
- 安装包资源包含 `onnx/model.onnx`、四个 tokenizer/config 文件、完整 MIT `LICENSE` 及 `NOTICE.md`。模型文件 SHA-256 为 `69A0B846F4F116B5E6AABF9546EA6754D02264F3211A13A1BD69B31B8040749A`，加载前会校验。
- `npm run tauri:build` 已生成 MSI 和 NSIS；MSI 反编译清单确认包含 `model.onnx`、`tokenizer.json`、`LICENSE` 和 `NOTICE.md`。MSI 为 70,238,208 字节，NSIS 为 54,074,233 字节。

## 验证

- Rust 全量：240 个库测试和 2 个契约测试通过。
- 真实内置模型回归：`汽车驶过城市街道` 与 `城市道路上的车辆` 的余弦相似度显著高于厨房切菜场景。
- 回归覆盖：Top-12 常量、清晰关键帧高于纯色帧、质量字段加载、历史 timeline 去重、新鲜素材优先、无效向量拒绝，以及内部向量不序列化给 Provider。
- Windows release、MSI、NSIS 构建通过；此前的 PDB 不兼容错误未复现。

## 文档同步

- `README.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/codebase/INTEGRATIONS.md`
- 本变更记录
