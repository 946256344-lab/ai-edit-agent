# 音乐先行剪辑：先选曲定窗口，切点落在节拍上

## 现象

2026-09-27「Weekend Road Trip」（16:9、BGM 开、配音关、要求 30 秒，素材库有 Mixkit `summer-fun.mp3`）：12 镜全是 2.5 秒，音乐在时间线生成后才从 0 秒铺上去，切点和节拍毫无关系，结尾硬截在任意位置。上一项改动（`2026-09-27-content-driven-shot-lengths.md`）让时长跟内容走，但仍不看音乐。

## 改动

### 导入时分析音乐（`assets/beats.rs`）

`kind = audio` 的素材在技术分析时用内置 FFmpeg 解码 22050Hz 单声道 PCM（最多前 6 分钟），Rust 本地计算，不引入新依赖：

1. 起音包络：一阶低通分出 <150Hz / 150–2500Hz / >2500Hz 三段，按 256 样本（约 11.6ms）求对数能量，正向差分相加，减去约 0.4 秒滑动均值后截负、按标准差归一。
2. 速度：包络在 60–200 BPM 滞后范围内的自相关系数，乘以 120 BPM 为中心、一个八度为标准差的对数高斯先验取峰，抛物线插值；峰值自相关系数作置信度（低于 0.1 不卡点）。
3. 跟拍：Ellis (2007) 动态规划（紧度 100），回溯后剪掉首尾起音过弱的节拍；BPM 取节拍间隔中位数。
4. 小节：按 4/4，四种相位里低频重音 + 0.5 × 总起音平均最强的为小节起点。
5. 乐句：每 4 小节，四种相位里小节能量与起音密度变化量平均最大的为乐句起点。
6. 能量曲线：每小节平均 RMS 的分贝值按 10%–95% 分位映射到 0–1。

结果写入 `metadata.beatAnalysis`（`version`、`durationMs`、`tempoBpm`、`confidence`、`beatsMs`、`downbeatsMs`、`phraseStartsMs`、`barEnergy`），当前 `AUDIO_ANALYSIS_VERSION = 1`。分析失败只记日志，不让素材失败。旧素材和 Jamendo 下载在被选为 BGM 时按需补算并写回（`ensure_beat_analysis`）。`summer-fun.mp3` 实测：114.9 BPM、置信度 0.27、小节约 2.09 秒，解码加分析约 1 秒。

### 选曲提前到分镜之前（`agentloop/auto_music.rs`）

BGM 开时 `generate_storyboard` 先选曲（素材库用户音频优先，其次 Jamendo 器乐，规则不变）并补节拍分析，再生成分镜。

### 配音关：音乐定时长（`music_plan.rs`、`storyboard/music_cuts.rs`）

- Phase 1 之后按目标时长（用户说的秒数，没说则 Phase 1 的目标）选音乐窗口：起点取乐句起点（其次第一个小节），终点取乐句边界或曲尾（其次小节起点），长度偏差不超过 25%，略偏好能量不低的起点。成片目标时长改为窗口长度，例如 120 BPM、8 秒一句时 30 秒请求得到 32 秒。
- Phase 4 按精修内容定好每镜时长（`fit_shots_to_content`）后，`snap_shots_to_music` 用动态规划把切点放到窗口内的拍上：每镜意向时长 = 内容偏好 ^0.7 × 能量对应小节数 ^0.3（高能量 1 小节、中 2 小节、低 4 小节），偏离按 3×(ln 比值)² 计；切在小节 / 乐句不罚，半小节 0.03、反拍 0.1；故事段落（beat）切换不在乐句边界罚 0.35；超过素材可用长度按放慢比例罚。镜头下限放不下时退到一拍，仍放不下就保留内容时长并记日志。源区间随新槽位重放（1 倍速，窗不够才放慢并写明）。
- 窗口写入分镜 `content_json.musicPlan`；不能卡点时写 `content_json.musicPlanNote`（真实原因），时长照旧按内容。
- 配乐时音乐从 `musicPlan.sourceStartMs` 起铺，结尾落在乐句结束，用最后一小节（0.8–2 秒）淡出；曲子自然结束则淡出 0.3 秒；窗口不从曲头开始时淡入 150ms。

### 配音开：旁白仍是时钟

时间线生成后选音乐起点，让成片结尾落在乐句边界或曲尾、起点尽量靠近小节起点；切点只在 ±120ms 内挪到最近的拍上，相邻镜头至少留 500ms，源区间按各自倍速同步移动（素材不够长就不动，倍速变化不到一成）。没有音乐窗口的无配音分镜同样走这条路。

### 预览与交付保持对齐

- 预览：每镜帧数改按起止点在时间线上的绝对帧位相减（`clip_frame_count`），切点误差不超过半帧、不随镜数累积；输入 `-t` 改到 `-i` 之前（原先写在 `-i` 之后实为输出时长，靠后一个 `-t` 覆盖，放慢镜头会少 0.1 秒）。缓存键加入帧数。
- FCPXML：主轨每镜时长按绝对帧位相减；旁白和音乐改为挂在第一镜下的连接片段（此前带 `lane` 直接放在 spine 里，不符合 FCPXML 结构，导入时可能被当成顺序片段），offset 按父片段本地时间。
- OTIO：旁白（A1）与音乐（A2）分轨，起点前用 Gap 补齐（此前两者同轨首尾相接，音乐会排在旁白后面）；循环音乐按源区间重复铺满，不再变速凑长度；1 倍速镜头时长按绝对帧位相减。
- 剪映 / CapCut：适配器本就按每段时间线起点与音乐 `sourceStartMs` 写入，未改。

### 给 Agent 的事实

`generate_storyboard` 结果新增 `musicTiming`：`mode`（`music_first` / `voiceover_clock` / `content_clock` / `not_beat_aligned`）、`tempoBpm`、`musicStartMs`、`endsOnPhrase`、`cutsOnBeat`、`cuts`，以及没能卡点时的 `note`。没有新增模型工具。

## 已知限制

- 小节只按 4/4 估计；三拍子、变速曲会卡错相位。乐句只按 4 小节。
- 局部编辑（`reselect_shots`、精修切点）锁原槽位，不重新吸附；派生分镜不带 `musicPlan`，时间线音乐轨沿用。
- FCPXML / OTIO 不写音乐淡入淡出；FCPXML 变速镜头 `timeMap` 的 `time` 从 0 起而 `start` 用源起点，可能取错画面（原有问题，已登记待查）。
- 窗口长度按乐句取整，可能比用户说的秒数长或短几秒（不超过 25%，且在分镜时长校验容差内）。

## 同步文档

`docs/api.md`、`docs/architecture.md`、`docs/decisions.md`、`docs/codebase/STRUCTURE.md`、`TASKS.md`。

## 验证

`cargo check` 通过；`cargo test --lib` 中 `assets::beats`（合成 120 BPM 点击轨：速度、节拍 ±25ms、小节、乐句；静音 / 过短拒绝）、`music_plan`（窗口、吸附到拍、段落落在乐句、镜数过多退到单拍、配音容差吸附）及 storyboard / preview / handoff / timeline / agentloop 共 318 条通过。待桌面实测：「Weekend Road Trip」同一简报重新生成，检查镜头时长分布、切点与节拍、音乐起点与结尾淡出，以及 CapCut 草稿 / FCPXML / OTIO 的音乐位置。
