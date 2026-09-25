// Agent 对话工作区：渲染消息、执行卡和 composer，所有状态与动作由 model/actions 注入。
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { FormEvent } from 'react'
import { AgentRunCard } from './AgentRunCard'
import { WorkspaceIcon } from './WorkspaceIcon'
import type { AssetAnalysisProgress as AnalysisProgress, MediaOptions, StoryboardVersion, StoredAgentTask } from '../lib/local-store'
import type { ConversationMessage, EditingSessionView } from './workspace-types'
import { useI18n } from '../lib/i18n'
import type { Messages } from '../lib/i18n'

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
  mediaOptions: MediaOptions
  analysis: { progress: AnalysisProgress; waiting: boolean; importing: boolean }
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
  toggleMedia: (name: keyof MediaOptions) => void
}

type AgentWorkspaceProps = {
  model: AgentWorkspaceModel
  actions: AgentWorkspaceActions
}

const mediaKeys: (keyof MediaOptions)[] = ['voiceover', 'subtitles', 'bgm']
const mediaIcons = { voiceover: 'microphone', subtitles: 'subtitles', bgm: 'music' } as const

function mediaSummary(options: MediaOptions, t: Messages) {
  return mediaKeys.map((key) => t.chat.mediaState(t.chat.media[key], options[key])).join(' · ')
}

export function AgentWorkspace({ model, actions }: AgentWorkspaceProps) {
  const stream = useRef<HTMLDivElement>(null)
  const composer = useRef<HTMLTextAreaElement>(null)
  const followLatest = useRef(true)
  const [showLatest, setShowLatest] = useState(false)
  const { analysis } = model
  const { t } = useI18n()
  const copy = t.chat

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
      <header className="chat-heading"><strong>{copy.heading}</strong><span>{model.isSending && !analysis.waiting ? copy.processing : ''}</span></header>
      <div className="message-stream" ref={stream} onScroll={(event) => {
        const element = event.currentTarget
        followLatest.current = element.scrollHeight - element.scrollTop - element.clientHeight < 80
        setShowLatest(!followLatest.current)
      }}>
        {!model.messages.length && <div className="session-intro">
          <span>FELLOWCUT</span>
          <strong>{model.storyboard?.title ?? model.session?.title ?? copy.introTitle}</strong>
          <p>
            {model.storyboard?.summary
              ?? model.session?.brief
              ?? copy.introBody}
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
            <button disabled={analysis.waiting} onClick={() => fillSuggestion(copy.suggestPromoPrompt)}>{copy.suggestPromo} <span aria-hidden="true">↗</span></button>
            <button disabled={analysis.waiting} onClick={() => fillSuggestion(copy.suggestPreparePrompt)}>{copy.suggestPrepare} <span aria-hidden="true">↗</span></button>
          </div>
        )}

        {model.messages.map((message) => {
          const mediaOptions = model.tasks.find((task) => task.input.userMessageId === message.id)?.input.mediaOptions
          return (
          <article key={message.id} className={`message ${message.role}`}>
            <div className="message-content">
              <div className="message-meta">
                {message.role === 'agent' ? 'FellowCut' : copy.you} <time>{message.time}</time>
              </div>
              <p>{message.content}</p>
              {mediaOptions && <small className="message-media-options">{copy.autoAdded} · {mediaSummary(mediaOptions, t)}</small>}
            </div>
          </article>
          )
        })}

        {model.tasks[0] && (
          <details className="agent-details" open={model.isSending}><summary>{model.isSending ? copy.viewProgress : copy.runRecord}</summary><AgentRunCard key={model.tasks[0].id} task={model.tasks[0]} onOpenStoryboard={actions.openArtifacts} /></details>
        )}
      </div>
      {showLatest && <button className="latest-message" onClick={() => {
        followLatest.current = true
        setShowLatest(false)
        stream.current?.scrollTo({ top: stream.current.scrollHeight })
      }}>{copy.backToLatest}</button>}

      <form className="composer" onSubmit={actions.sendMessage}>
        <textarea
          ref={composer}
          aria-label={copy.composerLabel}
          value={model.input}
          readOnly={analysis.waiting}
          onChange={(event) => actions.setInput(event.target.value)}
          placeholder={copy.composerPlaceholder}
          rows={2}
          onKeyDown={(event) => {
            if (event.key !== 'Enter' || event.shiftKey || event.nativeEvent.isComposing) {
              return
            }
            event.preventDefault()
            if (model.isSending || model.editBusy || analysis.importing || !model.input.trim()) {
              return
            }
            event.currentTarget.form?.requestSubmit()
          }}
        />
        <div className="composer-footer">
          <div className="composer-media-options" role="group" aria-label={copy.autoAdded}>
            {mediaKeys.map((key) => (
              <button key={key} type="button" aria-pressed={model.mediaOptions[key]}
                disabled={analysis.waiting}
                title={copy.mediaToggleTitle(model.mediaOptions[key], copy.media[key])}
                onClick={() => actions.toggleMedia(key)}>
                <WorkspaceIcon name={mediaIcons[key]} />
                <span className="composer-media-label">{copy.media[key]}</span>
                <span className="composer-media-check" aria-hidden="true"><WorkspaceIcon name="check" /></span>
              </button>
            ))}
          </div>
          {model.isSending ? (
            <button
              className="send-button send-button--stop"
              type="button"
              onClick={actions.stopAgentRun}
            >
              {analysis.waiting ? t.common.cancel : model.listenerReady ? copy.stop : copy.stopConnecting}
            </button>
          ) : (
            <button className={`send-button${analysis.importing ? ' send-button--analysis' : ''}`} type="submit" aria-label={model.editBusy ? t.common.saving : copy.send} disabled={!model.input.trim() || model.editBusy || analysis.importing}>
              {model.editBusy ? '…' : analysis.importing ? copy.importing : <WorkspaceIcon name="arrow" />}
            </button>
          )}
        </div>
        {model.composerNotice && <span role="status" className="composer-notice">{model.composerNotice}</span>}
      </form>
      <small className="composer-shortcut">{copy.shortcut}</small>
    </section>
  )
}
