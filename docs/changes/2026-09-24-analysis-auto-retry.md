# 分析失败自动补跑与不足 6 段送审

瞬时失败不再停在失败栏。画面识别按硬切段最多 6 段一批，最后剩余 1–5 段也立即送审。

## 结果

- 画面识别遇到超时、网络、空响应、Provider 暂不可用等瞬时错误时，自动重新入队；每条素材最多补 3 次，用尽才保持失败。
- 技术分析遇到超时或进程拉不起时最多补 2 次；源文件不在或用户取消不补。
- 用户跳过、不适用、任务 payload 无效和 4xx（除 429）不自动补。
- 用户或 Agent 点重试时清零补跑次数。
- 启动恢复和视觉队列空闲时，收走仍可补的失败/跳过项；4 条也发，不攒满 6 条。
- Windows `10060` /「没有正确答复」归为 `provider_timeout`，与英文 timeout 同一条路径。

## 范围

- `src-tauri/src/assets/retry.rs`：补跑策略与次数
- `src-tauri/src/assets/visual.rs`、`analysis.rs`：失败收尾、启动恢复
- `src-tauri/src/provider.rs`：10060 归类
- `src-tauri/src/models.rs`：`visualAnalysisRetryCount` / `analysisRetryCount`
- 同步 `docs/architecture.md`、`docs/api.md`、`docs/codebase/STRUCTURE.md`、`TASKS.md`

## 禁止变化

- 不改公开 Tauri 命令或 SQLite schema
- 不自动切换 Provider
- 不重跑用户显式跳过的画面识别
- 不加视觉 worker
