# Jamendo 配置入口与 API 错误

Provider 设置增加 Jamendo Client ID 输入框，调用已有 `save_jamendo_client_id` 命令，将值只保存到 Windows Credential Manager。界面只显示保存状态，不回读 Client ID，也不把“已保存”当作搜索成功。

Jamendo API 的 HTTP 200 响应仍可能在 JSON `headers.status` 中报告失败。搜索和按曲目 ID 复查现在检查该状态并返回错误码，避免将暂停的应用误判为“无匹配歌曲”。2026-09-26 实测官方公开测试 Client ID `709fa152` 返回 `code=11`（应用已暂停），因此没有用它配置本机。真实搜索、下载、写入时间线和预览仍需可用的自有 Client ID 验收。

同步文档：`docs/api.md`、`TASKS.md`。
