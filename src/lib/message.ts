// 消息格式转换：StoredMessage → ConversationMessage

import type { StoredMessage } from './local-store'
import type { ConversationMessage } from '../components/workspace-types'
import { formatClockTime } from './i18n'

export function toMessage(message: StoredMessage): ConversationMessage {
  return {
    id: message.id,
    role: message.role === 'user' ? 'user' : 'agent',
    content: message.content,
    time: formatClockTime(message.createdAt),
  }
}
