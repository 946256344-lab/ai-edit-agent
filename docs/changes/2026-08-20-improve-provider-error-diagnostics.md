# 2026-08-20: 改进 Provider 错误诊断与用户反馈

## 问题

用户报告经常出现"无法准备当前剪辑任务，请重试或重新选择项目"错误，但没有具体原因说明。错误链路分析：

1. 用户发送消息 → `App.tsx:sendMessage()`
2. 任务归属解析 → `resolveMessageContext()` → `resolveConversationTask()` (Tauri 命令)
3. Rust 端任务路由 → `taskrouter.rs:55-95` 的 `resolve_conversation_task()`
4. Provider 凭据检查 → `taskrouter.rs:95`: `ModelAccess::resolve().map_err(|_| "Task resolver model is unavailable.")`
5. 凭据解析失败 → `provider.rs:181-203` 的 `ModelAccess::resolve()` 返回 `Err`
6. 前端捕获错误 → `App.tsx:381-388` 的 `catch` 块显示通用错误消息

**核心问题**：

- `taskrouter.rs:95` 丢弃了原始错误 (`map_err(|_| ...)`)，只返回通用消息
- `App.tsx:381` 的 `catch` 块不捕获错误对象 (`catch {` 而非 `catch (error) {`)
- `ModelAccess::resolve()` 失败时没有日志记录

导致用户无法知道具体是：
- 自定义 API 凭据读取失败
- 自定义 API 未配置且 OAuth 未登录
- OAuth 已过期
- 还是其他问题

## 修复

### 1. 前端错误捕获与诊断 (`src/App.tsx:381-408`)

- 修改 `catch` 块为 `catch (error)`，捕获完整错误对象
- 提取 `errorMessage`，根据特征字符串提供具体诊断：
  - `"Task resolver model is unavailable"` → "AI 模型服务暂时不可用。请检查自定义 API 配置或 OAuth 登录状态。"
  - `"Custom API credential read failed"` → "自定义 API 凭据读取失败，请检查配置文件。"
  - `"OAuth not logged in"` → "OAuth 未登录或已过期，请重新登录。"
  - `"Current local project could not be verified"` → "当前项目不存在或已损坏，请重新选择项目。"
  - `"Task Resolver did not"` → 显示完整错误消息
- 添加 `console.error('[App] sendMessage failed:', errorMessage, error)` 以便浏览器控制台诊断

### 2. Rust Provider 日志 (`src-tauri/src/provider.rs:180-211`)

在 `ModelAccess::resolve()` 的四个分支添加日志：

- **自定义 API 成功**：`log::info!("[ModelAccess] Resolved to Custom API: base_url={}")`
- **OAuth 成功**：`log::info!("[ModelAccess] Resolved to OAuth (Custom API not configured)")`
- **自定义 API 凭据读取失败**：`log::error!("[ModelAccess] {}")`（包含完整错误）
- **OAuth 失败**：`log::error!("[ModelAccess] Custom API not configured, OAuth failed: {}")`

### 3. TaskRouter 错误透传 (`src-tauri/src/taskrouter.rs:95-98`)

修改 `ModelAccess::resolve()` 的错误处理：

```rust
// 修改前
let access = ModelAccess::resolve()
    .map_err(|_| "Task resolver model is unavailable.".to_owned())?;

// 修改后
let access = ModelAccess::resolve().map_err(|original_error| {
    log::error!("[TaskRouter] ModelAccess::resolve() failed: {}", original_error);
    format!("Task resolver model is unavailable: {}", original_error)
})?;
```

保留原始错误信息并添加前缀，同时记录到日志。

## 影响范围

- **前端**：`src/App.tsx` 错误处理逻辑，用户可见错误消息更具体
- **后端**：`src-tauri/src/provider.rs` 和 `src-tauri/src/taskrouter.rs` 增强日志，错误消息更完整
- **公开契约**：不变（Tauri 命令签名、参数、返回值不变）
- **持久化**：不变
- **用户体验**：改进（错误消息从通用变为具体，控制台和日志有完整诊断信息）

## 验证

- ✅ 163 个 Rust 库测试通过
- ✅ 前端 lint 通过
- ✅ `cargo check` 通过（20 个 unused 警告与本次修改无关）

## 后续建议

考虑添加一个前端"诊断"按钮，让用户一键检查：
- 自定义 API 配置状态
- OAuth 登录状态
- 当前项目有效性
- 素材库健康状态

这样用户无需手动阅读错误消息就能快速定位问题。
