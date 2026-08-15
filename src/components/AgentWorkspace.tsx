import type { FormEvent } from 'react'
import { AgentRunCard } from './AgentRunCard'
import type { StoryboardVersion, StoredAgentTask } from '../lib/local-store'
import type { ConversationMessage, EditingSessionView } from './workspace-types'

export type AgentWorkspaceModel = {
  session: Pick<EditingSessionView, 'id' | 'conversationId' | 'title' | 'brief'> | undefined
  storyboard: StoryboardVersion | null
  messages: ConversationMessage[]
  tasks: StoredAgentTask[]
  input: string
  isSending: boolean
  listenerReady: boolean
  composerNotice: string | null
  routeStatus: {
    text: string | null
    detail: string | null
    tone: 'neutral' | 'info' | 'success' | 'warning'
  }
}

export type AgentWorkspaceActions = {
  setInput: (value: string) => void
  openArtifacts: () => void
  sendMessage: (event: FormEvent<HTMLFormElement>) => void
}

type AgentWorkspaceProps = {
  model: AgentWorkspaceModel
  actions: AgentWorkspaceActions
}

export function AgentWorkspace({ model, actions }: AgentWorkspaceProps) {
  return (
    <section className="conversation-workspace conversation-workspace--chat">
      <div className="message-stream">
        <div className="session-intro">
          <span>当前剪辑会话</span>
          <strong>{model.storyboard?.title ?? model.session?.title ?? '从一句话开始剪辑'}</strong>
          <p>{model.storyboard?.summary ?? model.session?.brief ?? '描述你想做的视频。Agent 会记录需求、分析本地素材，并将 storyboard、内部时间线、Jianying draft 和 preview 作为可检查的工具结果。'}</p>
          {model.routeStatus.text && (
            <p
              className={`route-status route-status-${model.routeStatus.tone}`}
              title={model.routeStatus.detail ?? undefined}
            >
              {model.routeStatus.text}
            </p>
          )}
        </div>

        {!model.messages.length && (
          <div className="empty-chat">
            <button onClick={() => actions.setInput('制作一条 30 秒的英文产品宣传片')}>制作 30 秒宣传片</button>
            <button onClick={() => actions.setInput('我应该先准备哪些素材？')}>我应该先准备什么？</button>
          </div>
        )}

        {model.messages.map((message) => (
          <article key={message.id} className={`message ${message.role}`}>
            <div className="message-avatar">{message.role === 'agent' ? 'A' : 'Y'}</div>
            <div className="message-content">
              <div className="message-meta">
                {message.role === 'agent' ? 'Assembly Agent' : '你'} <time>{message.time}</time>
              </div>
              <p>{message.content}</p>
            </div>
          </article>
        ))}

        {model.tasks[0] && (
          <AgentRunCard key={model.tasks[0].id} task={model.tasks[0]} onOpenStoryboard={actions.openArtifacts} />
        )}
      </div>

      <form className="composer" onSubmit={actions.sendMessage}>
        <textarea
          value={model.input}
          onChange={(event) => actions.setInput(event.target.value)}
          placeholder="描述目标、提问或下达剪辑指令..."
          rows={2}
        />
        <div>
          <span className={model.composerNotice ? 'composer-notice' : undefined}>
            {model.composerNotice ?? (model.session ? `当前会话：${model.session.title}` : '首次发送将创建 local project 和剪辑会话')}
          </span>
          <button className="send-button" type="submit" disabled={model.isSending}>
            {model.isSending ? (model.listenerReady ? '处理中' : '连接中') : '发送'}
          </button>
        </div>
      </form>
    </section>
  )
}
