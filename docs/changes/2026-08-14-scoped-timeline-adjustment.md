# 2026-08-14 受限 timeline 调整与负向副作用边界

## 目标与边界

以真实桌面中已验证的 timeline v5 为基线，只把第 2 镜头从 3000 ms 缩短为 2500 ms，保持源起点 250 ms 和其他镜头、素材、顺序、文本轨、音乐轨不变。目标是验证单一新版本、对应 local preview、旧版本保留、播放与重启恢复。不得创建 Jianying draft、最终导出、删除或重新分析素材。

## 真实桌面结果

- 基线为 v5/v4/v3 共 3 个版本，v5 有 8 个片段、总时间线 32189 ms，v5 preview 文件存在。
- 第一次自然语言调整选择了 `get_timeline → change_clip_duration → finish`，但模型参数未通过后端校验；持久化诊断只保留安全码 `skill_execution_failed`，因此不推测更细原因。任务安全失败，timeline、preview 和操作日志均未改变。
- 第二次请求绑定 v5 的真实 timeline ID，并只允许 `shotIndex=2`、`newDurationMs=2500`、`newSourceStartMs=250`。`change_clip_duration` 只创建 v6：第 2 镜头源范围变为 250–2750 ms，后续镜头时间位置统一前移 500 ms，其他素材 ID、镜头时长和顺序不变。
- timeline 数量 3→4，版本为 v6/v5/v4/v3。v5 preview（3667496 bytes）与 v6 preview（3606411 bytes）同时存在，没有覆盖或删除旧文件。
- v6 preview 为 540×960、29.3 秒，真实播放器进度可以前进；Tauri 重启和 WebView 刷新后，v6、preview 与 27 条消息恢复。

## 发现的越界问题

第二次请求明确写了“不生成 preview、不创建 Jianying draft、不分析素材”，但旧 `fast_goal` 只按 deliverable 词命中顺序判断。因为文本中出现 `preview`，整轮目标被锁成 Preview；timeline v6 已成功后，完成门把它当成中间产物，模型随后两次选择 `render_preview`，第一次失败、第二次成功。

这不是模型创作选择错误，而是权限边界错误：否定条件只存在于自然语言 prompt，没有进入确定性目标与技能执行门。完成门因此反向扩大了副作用。

## 修复

- 新增请求级 `RequestToolPolicy`。它只收窄权限，不替模型选择正向工具。
- 明确否定 preview、Jianying draft 或素材分析时，分别禁用 `render_preview`、`create_jianying_draft`、`request_asset_analysis`；素材分析排除同时禁用会下载媒体并触发分析的 `download_music` 与 `use_online_music`。
- “只读/readonly”按分句解释；请求禁用全部编辑与交付工具，只保留观察工具，并阻止模型 `taskBrief` 在技能守卫前写回任务。中英文分号、冒号、逗号、句号、问叹号、换行与长破折号均隔离各自的正负范围。
- `fast_goal` 忽略被否定的 deliverable，且只锁定带明确动作的产物请求；名词/状态短句及未覆盖的新动作留给同一首轮主模型声明目标。Conversation Router 从首步工具目录过滤并拒绝被排除项，也拒绝把依赖被禁工具的 deliverable 声明为目标。
- 首次模型目标声明、路由复用的初始技能和后续每一步在真正执行前再次检查；越界选择不执行副作用，也不得重试同一被禁工具。越界工具以 `user_restricted_tool` 安全失败，越界目标只记录固定安全诊断。
- Agent `list_assets` 改用无调度持久化快照；Agent `generate_storyboard` 只消费已就绪证据。两者不再通过“观察”或 storyboard 生成旁路唤醒、提权或等待素材分析。
- Router 因被禁目标/工具等无效输出而回退到无首步 loop 时，显式只读和当前项目事实问题仍恢复至少一次成功观察的完成门；不能直接 `finish` 返回未经项目状态约束的答案。

## 修复后验收

- 单元测试覆盖中英文负向 preview/Jianying/素材分析、触发分析的媒体获取工具、只读目标/全写工具禁用、否定只读模式的假阳性排除、路由回退项目事实识别和正向 preview 不受影响。
- 真实桌面发送“只读检查当前 timeline，且不要生成 preview/Jianying/素材分析或修改产物”。任务只执行 `get_timeline → finish`，两步均完成，操作日志为 0。
- timeline 仍为 v6/v5/v4/v3，最新 timeline ID 与 preview 绑定不变；v6 preview 保持 3606411 bytes，修改时间仍为首次渲染时间。
- 后端与可见消息同步为 29，conversation 为 ready，composer 无错误提示。
- 未创建 Jianying draft、未最终导出、未删除或重新分析素材。已意外生成的 v6 preview 属于本地可恢复产物，未擅自删除。

## 验证命令

- `cargo fmt --all -- --check`
- `cargo test`（119 个单元测试 + 2 个 Agent 契约测试）
- `npm run lint`
- `npm run build`
- `npm run harness:check`
- `git diff --check`

## 同步文档

- `AGENTS.md`
- `TASKS.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/roadmap.md`
