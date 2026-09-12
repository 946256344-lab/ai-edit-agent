// Agent 对话工作区：渲染消息、执行卡和 composer，所有状态与动作由 model/actions 注入。
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
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
  editBusy?: boolean
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
  const stream = useRef<HTMLDivElement>(null)
  const composer = useRef<HTMLTextAreaElement>(null)
  const followLatest = useRef(true)
  const [showLatest, setShowLatest] = useState(false)

  useLayoutEffect(() => {
    const textarea = composer.current
    if (textarea) {
      textarea.style.height = 'auto'
      textarea.style.height = `${Math.min(textarea.scrollHeight, 180)}px`
    }
  }, [model.input])

  useEffect(() => {
    followLatest.current = true
    setShowLatest(false)
  }, [model.session?.id])

  useEffect(() => {
    if (followLatest.current && stream.current) {
      stream.current.scrollTop = stream.current.scrollHeight
    }
  }, [model.messages, model.tasks, model.isSending, model.session?.id])

  function fillSuggestion(value: string) {
    actions.setInput(value)
    composer.current?.focus()
  }

  return (
    <section className="conversation-workspace conversation-workspace--chat">
      <header className="chat-heading"><strong>剪辑助手</strong><span>{model.isSending ? '正在处理…' : '用对话完成粗剪'}</span></header>
      <div className="message-stream" ref={stream} onScroll={(event) => {
        const element = event.currentTarget
        followLatest.current = element.scrollHeight - element.scrollTop - element.clientHeight < 80
        setShowLatest(!followLatest.current)
      }}>
        {!model.messages.length && <div className="session-intro">
          <span>ASSEMBLY</span>
          <strong>{model.storyboard?.title ?? model.session?.title ?? '从一句话开始剪辑'}</strong>
          <p>
            {model.storyboard?.summary
              ?? model.session?.brief
              ?? '描述你的视频，我来完成粗剪。满意后交给剪映继续精修。'}
          </p>
          {model.routeStatus.text && (
            <p
              className={`route-status route-status-${model.routeStatus.tone}`}
              title={model.routeStatus.detail ?? undefined}
            >
              {model.routeStatus.text}
            </p>
          )}
        </div>}

        {!model.messages.length && (
          <div className="empty-chat">
            <button onClick={() => fillSuggestion('制作一条 30 秒的英文产品宣传片')}>制作 30 秒宣传片 <span aria-hidden="true">↗</span></button>
            <button onClick={() => fillSuggestion('我应该先准备哪些素材？')}>我应该先准备什么？ <span aria-hidden="true">↗</span></button>
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
          <details className="agent-details" open={model.isSending}><summary>{model.isSending ? '查看处理进度' : '本轮处理记录'}</summary><AgentRunCard key={model.tasks[0].id} task={model.tasks[0]} onOpenStoryboard={actions.openArtifacts} /></details>
        )}
      </div>
      {showLatest && <button className="latest-message" onClick={() => {
        followLatest.current = true
        setShowLatest(false)
        stream.current?.scrollTo({ top: stream.current.scrollHeight })
      }}>↓ 回到最新消息</button>}

      <form className="composer" onSubmit={actions.sendMessage}>
        <textarea
          ref={composer}
          aria-label="剪辑需求或文案"
          value={model.input}
          onChange={(event) => actions.setInput(event.target.value)}
          placeholder="描述想要的视频，或粘贴你的文案…"
          rows={2}
          onKeyDown={(event) => {
            if (event.key !== 'Enter' || event.shiftKey || event.nativeEvent.isComposing) {
              return
            }
            event.preventDefault()
            if (model.isSending || model.editBusy || !model.input.trim()) {
              return
            }
            event.currentTarget.form?.requestSubmit()
          }}
        />
        <div>
          <span role="status" className={model.composerNotice ? 'composer-notice' : undefined}>
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
            <button className="send-button" type="submit" disabled={!model.input.trim() || model.editBusy}>
              {model.editBusy ? '保存中…' : '发送'}
            </button>
          )}
        </div>
        <small className="composer-shortcut">Enter 发送 · Shift + Enter 换行</small>
      </form>
    </section>
  )
}
