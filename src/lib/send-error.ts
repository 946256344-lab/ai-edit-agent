// 发送失败提示：把任务归属 / 提交回合的底层错误映射为具体原因，不再只给笼统的“请重试”。
// 优先认 Rust 加的稳定码前缀（`provider_timeout: ...`），旧路径没有码时再按原文特征兜底；
// 兜底提示附带脱敏摘录：去掉 URL 查询串与账号、凭据、本机路径，并截断长度。
import { messages } from './i18n'

const CODE_PREFIX = /^(provider_[a-z0-9_]+):\s*/
const EXCERPT_LIMIT = 160

type ProviderFailure =
  | { kind: 'timeout' }
  | { kind: 'network' }
  | { kind: 'http'; status: number }
  | { kind: 'empty' }

export function describeSendError(rawError: string, hasContext: boolean): string {
  const errorCopy = messages().app.errors
  const codeMatch = CODE_PREFIX.exec(rawError)
  const errorMessage = codeMatch ? rawError.slice(codeMatch[0].length) : rawError
  const detail = sanitizeErrorExcerpt(errorMessage)

  if (errorMessage.includes('Task resolver model is unavailable')) return errorCopy.modelUnavailable
  if (errorMessage.includes('Custom API credential read failed')) return errorCopy.credentialFailed
  if (errorMessage.includes('OAuth not logged in')) return errorCopy.oauthExpired
  if (errorMessage.includes('Current local project could not be verified')) return errorCopy.projectMissing
  if (errorMessage.includes('Task Resolver did not')) return errorCopy.routeFailed(detail)

  const failure = codeMatch ? failureFromCode(codeMatch[1]) : failureFromText(errorMessage)
  if (failure) {
    switch (failure.kind) {
      case 'timeout': return errorCopy.providerTimeout(detail)
      case 'network': return errorCopy.providerNetwork(detail)
      case 'empty': return errorCopy.providerEmpty
      case 'http':
        if (failure.status === 401 || failure.status === 403) return errorCopy.providerAuth(failure.status)
        if (failure.status === 429) return errorCopy.providerRateLimited
        if (failure.status >= 500) return errorCopy.providerServerError(failure.status)
        return errorCopy.providerRejected(failure.status, detail)
    }
  }

  const summary = hasContext ? errorCopy.requestFailed : errorCopy.prepareFailed
  return detail ? errorCopy.withCause(summary, detail) : summary
}

// 与 src-tauri/src/provider.rs 的 classify_model_request_failure 码一一对应；provider_unknown 走兜底摘录。
function failureFromCode(code: string): ProviderFailure | null {
  if (code === 'provider_timeout') return { kind: 'timeout' }
  if (code === 'provider_network') return { kind: 'network' }
  if (code === 'provider_empty_response') return { kind: 'empty' }
  const http = /^provider_http_(\d{3})$/.exec(code)
  return http ? { kind: 'http', status: Number(http[1]) } : null
}

// 没有稳定码的旧错误：只认 Provider 传输层的固定写法，其余交给兜底摘录。
function failureFromText(error: string): ProviderFailure | null {
  const http = /:HTTP (\d{3})\b/.exec(error)
  if (http) return { kind: 'http', status: Number(http[1]) }
  if (/timed out|timeout|os error 10060/i.test(error) || /超时|没有正确答复|连接尝试失败/.test(error)) return { kind: 'timeout' } // i18n-allow: 匹配 Rust / Windows 传输错误原文
  if (/connection (failed|refused|reset)|os error 1006[14]|dns/i.test(error) || /网络错误|读取响应失败/.test(error)) return { kind: 'network' } // i18n-allow: 匹配 Rust 传输错误原文
  if (/response was empty/i.test(error) || /返回空响应体|响应为空/.test(error)) return { kind: 'empty' } // i18n-allow: 匹配 Rust 空响应原文
  return null
}

export function sanitizeErrorExcerpt(error: string): string {
  const cleaned = error
    .replace(/\bhttps?:\/\/[^\s，。、；）)"'<>]+/gi, safeUrl)
    .replace(/\bBearer\s+[^\s,;"')]+/gi, 'Bearer [redacted]')
    .replace(/\b(sk|ak|pk)-[A-Za-z0-9_-]{8,}/g, '[redacted]')
    .replace(/\b(api[_-]?key|access[_-]?token|token|secret|password|authorization)(\s*[=:]\s*)[^\s,;&"')]+/gi, '$1$2[redacted]')
    .replace(/\b[A-Za-z]:\\[^\s，。、；）)"'<>]*/g, (path) => `…\\${path.split('\\').filter(Boolean).pop() ?? ''}`)
    .replace(/\s+/g, ' ')
    .trim()
  return cleaned.length > EXCERPT_LIMIT ? `${cleaned.slice(0, EXCERPT_LIMIT)}…` : cleaned
}

// 只保留协议、主机、端口和路径：查询串与片段常带 key，账号密码也不外显。
function safeUrl(raw: string): string {
  try {
    const url = new URL(raw)
    return `${url.protocol}//${url.host}${url.pathname === '/' ? '' : url.pathname}`
  } catch {
    return raw.replace(/[?#].*$/, '').replace(/\/\/[^/@]*@/, '//')
  }
}
