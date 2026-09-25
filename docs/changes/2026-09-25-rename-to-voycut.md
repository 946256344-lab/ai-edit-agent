# 2026-09-25：对外品牌改名 Voycut

## 结果

桌面应用对外名称由 FellowCut 改为 Voycut，写法按品牌设计稿字标（仅首字母大写）。窗口标题、安装包名（`productName`）、侧栏字标、会话开场标记、助手消息署名、浏览器模式提示、OAuth 回调页、剪映/CapCut 草稿与 FCPXML 的默认名称都随之改为 Voycut。

## 不变的部分

- 应用标识 `com.assembly.videoagent`、数据库文件名、凭据服务名、npm/Cargo 包名不变，保证读取本机已有项目与凭据。
- 界面语言偏好的本机存储键 `fellowcut.locale` 不变，避免用户已选语言丢失。
- 应用图标与 favicon 暂不更换，等设计稿源文件（矢量或高清 PNG）到位后再生成全套图标。

## 触发范围

- `index.html`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`、`README.md`。
- `src/components/AppSidebar.tsx`、`src/components/AgentWorkspace.tsx`、`src/lib/i18n/{zh-CN,en}.ts`。
- `src-tauri/src/{jianying.rs,oauth.rs,handoff/deliver.rs,handoff/fcpxml.rs,agentloop/skills.rs}`、`src-tauri/scripts/create_jianying_draft.py`。
- 发行验收脚本 `scripts/verify-*.mjs` 按新窗口标题查找 WebView。

## 验证

见 `TASKS.md` 对应条目。
