// Agent 对话工作区：渲染消息、执行卡和 composer，所有状态与动作由 model/actions 注入。
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
  stopAgentRun: () => void
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
          <span>当前项目</span>
          <strong>{model.storyboard?.title ?? model.session?.title ?? '从一句话开始剪辑'}</strong>
          <p>
            {model.storyboard?.summary
              ?? model.session?.brief
              ?? '告诉我你想剪什么，我会分析素材并生成第一版视频。'}
          </p>
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
                {message.role === 'agent' ? 'Assembly' : '你'} <time>{message.time}</time>
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
          placeholder="想剪成什么？直接说需求或文案…（Enter 发送，Shift+Enter 换行）"
          rows={2}
          onKeyDown={(event) => {
            if (event.key !== 'Enter' || event.shiftKey || event.nativeEvent.isComposing) {
              return
            }
            event.preventDefault()
            if (model.isSending || !model.input.trim()) {
              return
            }
            event.currentTarget.form?.requestSubmit()
          }}
        />
        <div>
          <span className={model.composerNotice ? 'composer-notice' : undefined}>
            {model.composerNotice ?? (model.session ? '可以说得更具体一点，例如时长、节奏或重点画面' : '发送后会自动创建项目并开始')}
          </span>
          {model.isSending ? (
            <button
              className="send-button send-button--stop"
              type="button"
              onClick={actions.stopAgentRun}
            >
              {model.listenerReady ? '停止' : '停止连接'}
            </button>
          ) : (
            <button className="send-button" type="submit" disabled={!model.input.trim()}>
              发送
            </button>
          )}
        </div>
      </form>
    </section>
  )
}
