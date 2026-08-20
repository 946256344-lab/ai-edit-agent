# 2026-08-20 ElevenLabs 文案转配音

## 结果

程序可以把明确旁白文案合成配音：密钥进 Credential Manager，ElevenLabs `with-timestamps` 出 MP3 和对齐时间，旁白作为时间线时钟，字幕只跟 alignment，preview 混音不再用 `-shortest` 截断口播。

## 行为

- 新命令：`get_elevenlabs_status`、`save_elevenlabs_api_key`、`clear_elevenlabs_api_key`、`import_elevenlabs_api_key_from_environment`。
- Native 工具：`list_voices`（观察）、`synthesize_voiceover`（写）。做剪辑默认授权配音；「不要配音」关闭且网络次数为 0。
- 文案只来自工具 `text` 或 storyboard `narrationText`，不朗读 `onScreenText`。用户没给文案时，storyboard 为每个 beat 写口播 `narrationText`，确认后按这条合成配音。确认流程里 ElevenLabs 配音失败不得丢掉已创建的时间线和预览；TTS 请求只发送文档规定的 `text` 与 `model_id`。
- 默认 Charlie；缺失则失败并返回可用音色，不静默换声。
- 指纹缓存相同请求；HTTP 超时不自动重试。
- 画面短于口播时追加 freeze-frame 派生段，不把 `sourceEndMs` 推过已验证源范围。画面过长只警告。
- 字幕 `origin`：只替换 `storyboard_generated` / `voice_alignment`。

## 后续修复

模型在一轮里同时调用 `get_timeline` 和 `list_voices` 时，两步共用 `step_number=1`，触发 `agent_run_steps` 唯一约束，循环在配音前中断。现改为每个工具独立递增步骤号；Chat 响应里仅空白内容加 tool_calls 时不再插入假 assistant 文本。

真机第二轮「用这个文案生成视频」只拿到观察工具，模型把整库 `list_assets` 塞进 16k 上下文后空转到 10 步。现「生成视频/配音」授权 storyboard、时间线和配音；Native 不得用 `list_assets` 拼镜头。`generate_storyboard` 先列 beats，再对每个 beat 按 `requiredVisual` 从全库取 5 个匹配预选、读关键帧网格后挑选，直到分镜满足或诚实留空。单镜 JSON/网络失败只留空该 beat，不再整份失败。规范化不再把有效源范围拉回片头；同一素材重叠窗口会切成不重叠片段，已有镜头的 beat 不会再留在 uncovered 列表。`list_assets` 仍在，只报库存，样本优先 ready。超大 tool output 先截断；可空参数把 `""` 当成 null；Chat 适配器把 assistant 文本与 function_call 合成一条消息；ElevenLabs 未配置返回 `voice_provider_*`。当前剪辑任务仍没有时间线，配音必须先生成并确认 storyboard。

## 不在本变更

旁白进 Jianying、第二家 TTS、保留镜头原声、按比例拉伸每一镜、与 ElevenLabs 账单对账。

## 同步文档

- `TASKS.md`
- `AGENTS.md`
- `README.md`
- `docs/architecture.md`
- `docs/api.md`
- `docs/decisions.md`
- `docs/harness.md`
- `docs/codebase/STRUCTURE.md`
- `docs/codebase/INTEGRATIONS.md`
- `src-tauri/src/AGENTS.md`
- `TASKS.md`
- `docs/changes/2026-08-20-elevenlabs-voiceover.md`

