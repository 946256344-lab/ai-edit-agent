# 目标优先视觉与 Provider 优化

2026-08-12

## 决策

- brief 在本地与显示名、文件夹组织 hint、OCR 做词汇重合评分。
- 只为 queued 视觉批次持久化纯数字 priority；无命中与同分按创建时间和任务 ID 稳定排序。
- 最高相关的 queued 或 running 批次最多等待 65 秒，之后使用已落地视觉证据，不等待全部素材。
- 文件名、文件夹和路径不进入 Provider；OCR 不进入粗视觉 Provider payload，但仍可作为明确标注的本地提取文字证据进入 storyboard，不能冒充画面语义。
- OAuth 与自定义 API 请求共享进程级 HTTP Agent，同时保留每次请求超时。
- 自定义 API 可配置可选粗视觉 Model；为空时沿用主 Model。OAuth 不使用未经验证的替代模型。

## 同步文档

- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `TASKS.md`

## 验证

- `cargo fmt --check`、`cargo test --lib`（76 通过）、`npm run lint`、`npm run build`、`npm run harness:check` 与 `git diff --check` 通过。
- 独立审查发现并修复运行中相关批次未等待、视觉任务非原子 claim、同毫秒排序不稳定、凭据状态错误重复包装和文档中 OCR 用途混淆。真实 Provider 延迟与粗视觉模型兼容性待桌面测量。
