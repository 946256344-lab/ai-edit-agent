// 发送后先显示临时用户消息，落库或取消后清除；真实会话仍以 SQLite 为准。
import { useState } from 'react'
import type { ConversationMessage } from '../components/workspace-types'

export function usePendingUserMessageController() {
  return useState<ConversationMessage | null>(null)
}
