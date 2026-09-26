// 账号弹窗错误：Rust 返回 `account_*: 中文原文`，这里按稳定码给出当前界面语言的原因；
// 未知码或没有码时显示码之后的原文。
import { messages } from './i18n'

const CODE_PREFIX = /^(account_[a-z_]+):\s*/

export function describeAccountError(reason: unknown): string {
  const raw = String(reason)
  const match = CODE_PREFIX.exec(raw)
  if (!match) return raw
  const known = messages().account.errors as Record<string, string | undefined>
  return known[match[1]] ?? raw.slice(match[0].length)
}
