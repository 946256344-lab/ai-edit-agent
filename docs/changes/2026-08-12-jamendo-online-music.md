# Jamendo 在线音乐 Provider

Jamendo 是可替换的线上音乐 Provider，不是随应用分发的内置曲库。应用只在 Agent 选定单曲后，从 Jamendo API 下载一份副本到当前 local project 的受控目录；不会爬取网站、批量缓存音乐，或把远程 URL 写入内部时间线和 Jianying draft。

`client_id` 只保存于 Windows Credential Manager。搜索和交付前都会重新校验 API 返回的许可：仅允许 `audiodownload_allowed` 的 CC0、CC-BY 3.0 或 CC-BY 4.0 曲目，拒绝 NC、ND 和其他许可。CC-BY 的艺术家、曲名与许可 URL 会保留在 `MusicCue` 的 attribution 元数据中。

`use_online_music` 是具名、可审计的受限 Agent 工具：它下载恰好一首合格曲目，等待既有本地媒体分析完成，再创建一个新的内部时间线版本，按完整 timeline 循环并设置安全的背景音量、淡入和淡出。文件名带随机标识，重选相同曲目也不会覆盖已有本地副本。它不执行最终导出，也不覆盖既有 Jianying draft。含音乐的 Jianying draft 仍是实验性产物，必须在 Jianying UI 中试听复核。

验证：`cargo test --lib`（66 通过，包含许可过滤与 attribution 单测）；真实 Jamendo catalog API 已返回可下载与许可字段。完整桌面端 Agent 到 Jianying 的试听验收为后续人工测试项。
