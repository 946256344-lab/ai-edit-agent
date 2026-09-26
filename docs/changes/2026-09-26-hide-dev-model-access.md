# 2026-09-26：正式版隐藏实验性 OAuth 与自定义 API

## 现象

正式版模型只走 Voycut 网关，OAuth 与自定义 API 实际用不到，但模型设置里仍显示「Sign in with ChatGPT」。文档把 OAuth 定位为「仅个人测试、非官方集成」，公开用户容易误以为 Voycut 冒用 OpenAI。

## 触发范围

- `src/components/ProviderSettingsModal.tsx`：OpenAI OAuth 与自定义 API 两个区块只在开发构建（`import.meta.env.DEV`）显示；正式版标题与说明改为「模型与配音」，说明登录后使用 Voycut 模型服务、这里无需连接，配音服务为可选的自带密钥。
- `src/hooks/useProviderController.ts`：正式版侧栏模型按钮的提示固定为「Voycut 模型服务」，不再显示「模型未连接」。
- `src/lib/i18n/zh-CN.ts`、`en.ts`：新增 `provider.releaseTitle`、`releaseIntro`、`labelGateway`。

## 改动

只改界面显示；Rust 命令、凭据存储和 `ModelAccess::resolve` 的网关优先顺序不变。开发构建显示与改动前一致。

## 同步文档

`docs/release-checklist.md`。

## 验证

`npx tsc -b`、`npm run lint`、`npm run i18n:check` 通过。正式版弹窗显示随安装包验收。

## 决策

无。配音区块是否保留等 D5 决定。
