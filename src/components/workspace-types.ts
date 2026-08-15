// 前端工作区共享的轻量视图模型；持久化事实类型仍以 local-store bridge 为准。
export type WorkspaceView = 'chat' | 'assets' | 'artifacts'

export type EditingSessionView = {
  id: string
  conversationId: string | null
  title: string
  preview: string
  brief: string
  updated: string
  state: 'ready' | 'working' | 'review'
}

export type ConversationMessage = {
  id: string
  role: 'agent' | 'user'
  content: string
  time: string
}

export type StoryboardAsset = {
  id: string
  name: string
}

export type AssetPageCounts = {
  total: number
  ready: number
  analyzing: number
  queued: number
  failed: number
}
