# 2026-09-26：网关错误改用稳定码，界面按语言显示

## 现象

Voycut 网关失败时，Rust 返回写死的中文句子（如「Voycut 使用资格不可用」），英文界面会直接显示中文；这些句子没有 `provider_*` 码，发送失败提示只能落到笼统兜底。

## 触发范围

- `src-tauri/src/provider.rs`：网关分支改为英文原文，专属原因带稳定码：`provider_gateway_auth`（未登录、令牌刷新失败或 401）、`provider_gateway_entitlement`（403）、`provider_gateway_payload_too_large`（413）。其他状态沿用 `...:HTTP {status}` 通用分类；连接失败、空响应分别归入 `provider_network`、`provider_empty_response`。`classify_model_request_failure` 与 `with_model_failure_code` 认已有码前缀，不再二次加码。
- `src/lib/send-error.ts`：识别三个网关码。
- `src/lib/i18n/zh-CN.ts`、`en.ts`：新增 `gatewayAuth`、`gatewayEntitlement`、`gatewayPayloadTooLarge`。
- `src-tauri/src/agentloop/native.rs`（后续提交）：回合中断的系统回复（停止、总超时、步骤上限、模型无法回复）按任务界面语言给出；模型失败是网关登录、资格或请求体过大时，把原因写在回复开头。

## 改动

- 公开命令签名不变；网关失败的错误字符串从中文句子变为 `code: English text` 或以 `:HTTP {status}` 结尾的英文句子。
- 网关连接失败的原文仍含 `connection`，分步重试对它的判定不变。

## 同步文档

`docs/release-checklist.md`。

## 验证

`cargo check`、`cargo test --lib agentloop::native`（新增英文网关失败回归）、`npx tsc -b`、`npm run lint`、`npm run harness:check` 通过。401 / 403 / 413 经真实网关的显示随 §1.4 失败路径验收。

## 决策

无。
