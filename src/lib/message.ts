// 消息格式转换：StoredMessage → ConversationMessage

import type { StoredMessage } from './local-store'
import type { ConversationMessage } from '../components/workspace-types'

export function toMessage(message: StoredMessage): ConversationMessage {
  return {
    id: message.id,
    role: message.role === 'user' ? 'user' : 'agent',
    content: message.content,
    time: new Date(message.createdAt).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' }),
  }
}
