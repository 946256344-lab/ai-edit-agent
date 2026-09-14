# 素材详情预览与分析展示

## 范围

- 修复存在场景片段时整条素材视觉标签被隐藏的问题。
- 详情顶部显示原视频、音频或图片；视频片段按源起止范围播放，可返回完整视频。
- 将四个统计卡压缩为一行，片段使用中文编号、代表帧和真实描述，OCR 默认折叠。
- 桌面详情栏宽 360–420px，窄窗口使用侧边覆盖面板，关闭按钮保留在面板内。
- 读取接口仅增加媒体类型和源路径，并精确授权选中的媒体文件。不改导入、分析、时间线或导出。
- 契约说明同步在 `docs/api.md`。
- 分支从 origin/master 建立，补齐当前开发版已有的“查看分析”入口接线；不会覆盖开发版其他未提交界面改动。

## 验证

- 前端 lint/build、分支策略、架构预算、文档同步、`git diff --check`、Rust fmt/check 通过。保留 StudioWorkspace 原有 const-comparisons 警告及 Rust 既有 dead-code 警告。
- 新增 `node scripts/test-asset-evidence.mjs`：有分段但无片段视觉标签时，素材级视觉结果仍显示，OCR 默认收起；通过。
- Rust 库测试 350/351 通过，新增 `asset_evidence_exposes_the_frontend_media_contract` 通过。未通过项为既有 `timeline::tests::every_advertised_text_recipe_is_accepted_by_the_text_track_validator` 的 subtitle_newsbar 失败。
- 2 项 Agent 集成契约测试通过。Cargo 集成测试命令被运行中的主程序 exe 文件锁阻挡复制，随后直接运行本轮编译生成的集成测试 exe，2/2 通过。
- 已有 harness:test 的文档规则断言失败；harness:check 的 outbound_http.rs 所有权规则失败。本轮未修改这两处规则或字幕实现。
- 使用当前开发版样式、真实 8 秒视频副本与真实分析结果，在独立本地页面验证实际组件。片段 2 从 3 秒播放至 6 秒后暂停；返回整片从 0 播放；OCR 展开与收起、关闭和重新打开状态重置通过，浏览器无 error/warn。
- 1024×650 详情宽 400px、贴靠右侧，1440×900 详情宽约 403px；均无横向溢出，关闭按钮可见。
- 该真实素材原始分析中没有片段关键帧与片段描述，因此诚实显示暂无片段描述；未生成或伪造缩略图、标签。
- `.artifacts/asset-detail-after.png` 保存独立预览截图；已同步源码并启动桌面开发版，主窗口正常打开。
