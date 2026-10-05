// Agent 对话工作区：渲染消息、执行卡和 composer，所有状态与动作由 model/actions 注入。
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { FormEvent } from 'react'
import { AgentRunCard } from './AgentRunCard'
import { MessageMarkdown } from './MessageMarkdown'
import { WorkspaceIcon } from './WorkspaceIcon'
import { BrandMark } from './BrandMark'
import { aspectRatios, genreSelections } from '../lib/local-store'
import type { AssetAnalysisProgress as AnalysisProgress, AspectRatio, GenreSelection, MediaOptions, StoryboardVersion, StoredAgentTask } from '../lib/local-store'
import type { MediaToggle } from '../hooks/useComposerMediaController'
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
  /** 网关明确没有配音能力时为 false，隐藏配音开关。 */
  voiceAvailable: boolean
  /** 当前输出编辑器的界面名称，空对话引导语按它说明交付去向。 */
  outputEditorLabel: string
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
  toggleMedia: (name: MediaToggle) => void
  setAspectRatio: (ratio: AspectRatio) => void
  setGenre: (genre: GenreSelection) => void
}

type AgentWorkspaceProps = {
  model: AgentWorkspaceModel
  actions: AgentWorkspaceActions
}

// 输入框下方顺序：画幅 · BGM · 配音；字幕随配音发送，不单独展示开关。
const mediaKeys: MediaToggle[] = ['bgm', 'voiceover']
const mediaIcons = { voiceover: 'microphone', bgm: 'music' } as const

function mediaSummary(options: MediaOptions, t: Messages) {
  return [
    t.chat.aspectState(options.aspectRatio ?? '9:16'),
    `${t.chat.genreLabel}: ${t.chat.genres[options.genre ?? 'auto']}`,
    ...mediaKeys.map((key) => t.chat.mediaState(t.chat.media[key], options[key])),
  ].join(' · ')
}

export function AgentWorkspace({ model, actions }: AgentWorkspaceProps) {
  const stream = useRef<HTMLDivElement>(null)
  const composer = useRef<HTMLTextAreaElement>(null)
  const followLatest = useRef(true)
  const [showLatest, setShowLatest] = useState(false)
  const { analysis } = model
  const { t } = useI18n()
  const copy = t.chat
  const stopLabel = analysis.waiting ? t.common.cancel : model.listenerReady ? copy.stop : copy.stopConnecting

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
      <div className="message-stream" ref={stream} onScroll={(event) => {
        const element = event.currentTarget
        followLatest.current = element.scrollHeight - element.scrollTop - element.clientHeight < 80
        setShowLatest(!followLatest.current)
      }}>
        {!model.messages.length && <div className="session-intro">
          <span>VOYCUT</span>
          <strong>{model.storyboard?.title ?? model.session?.title ?? copy.introTitle}</strong>
          <p>
            {model.storyboard?.summary
              ?? model.session?.brief
              ?? copy.introBody(model.outputEditorLabel)}
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
                {message.role === 'agent' ? <><BrandMark className="brand-mark message-avatar" />Voycut</> : copy.you} <time>{message.time}</time>
              </div>
              {message.role === 'agent' ? <MessageMarkdown content={message.content} /> : <p>{message.content}</p>}
              {mediaOptions && <small className="message-media-options">{copy.autoAdded} · {mediaSummary(mediaOptions, t)}</small>}
            </div>
          </article>
          )
        })}

        {model.tasks[0] && (
          <AgentRunCard key={model.tasks[0].id} task={model.tasks[0]} onOpenStoryboard={actions.openArtifacts} />
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
            <label className="composer-ratio" title={copy.aspectTitle}>
              <WorkspaceIcon name="aspect" />
              <span className="composer-media-label">{model.mediaOptions.aspectRatio ?? '9:16'}</span>
              <select aria-label={copy.aspectLabel} value={model.mediaOptions.aspectRatio ?? '9:16'}
                disabled={analysis.waiting}
                onChange={(event) => actions.setAspectRatio(event.target.value as AspectRatio)}>
                {aspectRatios.map((ratio) => <option key={ratio} value={ratio}>{copy.aspectOption(ratio)}</option>)}
              </select>
            </label>
            <label className="composer-ratio" title={copy.genreTitle}>
              <span className="composer-media-label">{copy.genres[model.mediaOptions.genre ?? 'auto']}</span>
              <select aria-label={copy.genreLabel} value={model.mediaOptions.genre ?? 'auto'}
                disabled={analysis.waiting}
                onChange={(event) => actions.setGenre(event.target.value as GenreSelection)}>
                {genreSelections.map((genre) => <option key={genre} value={genre}>{copy.genres[genre]}</option>)}
              </select>
            </label>
            {mediaKeys.filter((key) => key !== 'voiceover' || model.voiceAvailable).map((key) => (
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
          <small className="composer-shortcut">{copy.shortcut}</small>
          {model.isSending ? (
            <button
              className="send-button send-button--stop"
              type="button"
              aria-label={stopLabel}
              title={stopLabel}
              onClick={actions.stopAgentRun}
            >
              <WorkspaceIcon name="stop" />
            </button>
          ) : (
            <button className={`send-button${analysis.importing ? ' send-button--analysis' : ''}`} type="submit" aria-label={model.editBusy ? t.common.saving : copy.send} disabled={!model.input.trim() || model.editBusy || analysis.importing}>
              {model.editBusy ? '…' : analysis.importing ? copy.importing : <WorkspaceIcon name="send" />}
            </button>
          )}
        </div>
        {model.composerNotice && <span role="status" className="composer-notice">{model.composerNotice}</span>}
      </form>
    </section>
  )
}
