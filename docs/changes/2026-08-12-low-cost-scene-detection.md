# 低成本场景检测

2026-08-12

## 决策

- 首次场景扫描仍限制前 30 秒。
- FFmpeg 滤镜顺序改为 `fps=4 -> scale=320 -> scene select -> showinfo`。
- 使用 fast bilinear 缩放，最多输出 4 张关键帧。
- `showinfo pts_time` 继续作为关键帧的源时间。

## 基准

本机 90 秒 1080p 测试视频，相同前 30 秒扫描窗口各运行三次：

- 旧链路：3647、3392、2912ms，平均 3317ms。
- 新链路：2798、2386、1655ms，平均 2280ms。
- 平均提升约 31.3%。

## 同步文档

- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `TASKS.md`

## 验证

- `cargo fmt --check`、`cargo test --lib`（69 通过）、`npm run lint`、`npm run build`、`npm run harness:check` 与 `git diff --check` 通过。
- 独立审查待执行。
