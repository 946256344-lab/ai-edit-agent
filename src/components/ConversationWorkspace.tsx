import type { FormEvent } from 'react'
import { convertFileSrc } from '@tauri-apps/api/core'
import { AgentAuditPanel } from './AgentAuditPanel'
import { AgentRunCard } from './AgentRunCard'
import type {
  PreviewResult,
  StoryboardVersion,
  StoredAgentTask,
  StoredOperationLog,
  TimelineVersion,
} from '../lib/local-store'

export type ConversationMessage = {
  id: string
  role: 'agent' | 'user'
  content: string
  time: string
}

export type ConversationSession = {
  id: string
  conversationId: string | null
  title: string
  brief: string
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

type ConversationWorkspaceProps = {
  activeEditingSession: ConversationSession | undefined
  activeView: 'chat' | 'artifacts'
  agentEditListenerReady: boolean
  agentTasks: StoredAgentTask[]
  assetCount: AssetPageCounts
  assets: StoryboardAsset[]
  composerNotice: string | null
  deliveryStatus: string
  input: string
  isCreatingJianyingDraft: boolean
  isCreatingTimeline: boolean
  isGeneratingStoryboard: boolean
  isRenderingPreview: boolean
  isSending: boolean
  messages: ConversationMessage[]
  onCreateJianyingDelivery: () => void
  onCreateTimelineFromStoryboard: () => void
  onGenerateStoryboard: () => void
  onInputChange: (value: string) => void
  onOpenStoryboard: () => void
  onRenderLocalPreview: () => void
  onSendMessage: (event: FormEvent<HTMLFormElement>) => void
  onSetActiveView: (view: 'chat' | 'artifacts') => void
  onStoryboardBriefChange: (value: string) => void
  preview: PreviewResult | null
  previewNonce: number
  routeStatusDetail: string | null
  routeStatusText: string | null
  routeStatusTone: 'neutral' | 'info' | 'success' | 'warning'
  setInput: (value: string) => void
  storyboard: StoryboardVersion | null
  storyboardBrief: string
  storyboardError: string | null
  timeline: TimelineVersion | null
  timelineVersions: TimelineVersion[]
  operationLogs: StoredOperationLog[]
}

export function ConversationWorkspace({
  activeEditingSession,
  activeView,
  agentEditListenerReady,
  agentTasks,
  assetCount,
  assets,
  composerNotice,
  deliveryStatus,
  input,
  isCreatingJianyingDraft,
  isCreatingTimeline,
  isGeneratingStoryboard,
  isRenderingPreview,
  isSending,
  messages,
  onCreateJianyingDelivery,
  onCreateTimelineFromStoryboard,
  onGenerateStoryboard,
  onInputChange,
  onOpenStoryboard,
  onRenderLocalPreview,
  onSendMessage,
  onSetActiveView,
  onStoryboardBriefChange,
  preview,
  previewNonce,
  routeStatusDetail,
  routeStatusText,
  routeStatusTone,
  setInput,
  storyboard,
  storyboardBrief,
  storyboardError,
  timeline,
  timelineVersions,
  operationLogs,
}: ConversationWorkspaceProps) {
  const formatEvidenceTime = (timeMs: number | null) => {
    if (timeMs === null) return '图片'
    const seconds = Math.floor(timeMs / 1000)
    return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
  }

  return (
    <section className={`conversation-workspace conversation-workspace--${activeView}`}>
      {activeView === 'chat' ? (
        <>
          <div className="message-stream">
            <div className="session-intro">
              <span>当前剪辑会话</span>
              <strong>{storyboard?.title ?? activeEditingSession?.title ?? '从一句话开始剪辑'}</strong>
              <p>{storyboard?.summary ?? activeEditingSession?.brief ?? '描述你想做的视频。Agent 会记录需求、分析本地素材，并将 storyboard、内部时间线、Jianying draft 和 preview 作为可检查的工具结果。'}</p>
              {routeStatusText && (
                <p className={`route-status route-status-${routeStatusTone}`} title={routeStatusDetail ?? undefined}>
                  {routeStatusText}
                </p>
              )}
            </div>
            {!messages.length && (
              <div className="empty-chat">
                <button onClick={() => setInput('制作一条 30 秒的英文产品宣传片')}>制作 30 秒宣传片</button>
                <button onClick={() => setInput('我应该先准备哪些素材？')}>我应该先准备什么？</button>
              </div>
            )}
            {messages.map((message) => (
              <article key={message.id} className={`message ${message.role}`}>
                <div className="message-avatar">{message.role === 'agent' ? 'A' : 'Y'}</div>
                <div className="message-content">
                  <div className="message-meta">{message.role === 'agent' ? 'Assembly Agent' : '你'} <time>{message.time}</time></div>
                  <p>{message.content}</p>
                </div>
              </article>
            ))}
            {agentTasks[0] && <AgentRunCard key={agentTasks[0].id} task={agentTasks[0]} onOpenStoryboard={onOpenStoryboard} />}
          </div>

          <form className="composer" onSubmit={onSendMessage}>
            <textarea
              value={input}
              onChange={(event) => onInputChange(event.target.value)}
              placeholder="描述目标、提问或下达剪辑指令..."
              rows={2}
            />
            <div>
              <span className={composerNotice ? 'composer-notice' : undefined}>
                {composerNotice ?? (activeEditingSession ? `当前会话：${activeEditingSession.title}` : '首次发送将创建 local project 和剪辑会话')}
              </span>
              <button className="send-button" type="submit" disabled={isSending}>
                {isSending ? (agentEditListenerReady ? '处理中' : '连接中') : '发送'}
              </button>
            </div>
          </form>
        </>
      ) : (
        <div className="artifact-stream">
          <section className="workflow-card artifact-workflow">
            <div className="workflow-card-header">
              <div>
                <span className="panel-kicker">当前成果</span>
                <strong>storyboard → timeline → preview → Jianying draft</strong>
              </div>
              <small>{deliveryStatus}</small>
            </div>
            <div className="workflow-actions">
              <button className="primary-button" onClick={() => onCreateTimelineFromStoryboard()} disabled={!storyboard || isCreatingTimeline}>
                {isCreatingTimeline ? '创建中' : timeline ? '新建时间线版本' : '创建时间线'}
              </button>
              <button className="outline-button" onClick={() => onRenderLocalPreview()} disabled={!timeline || isRenderingPreview}>
                {isRenderingPreview ? '生成中' : preview ? '重新生成 preview' : '生成 preview'}
              </button>
              <button className="outline-button" onClick={() => onCreateJianyingDelivery()} disabled={!timeline || isCreatingJianyingDraft}>
                {isCreatingJianyingDraft ? '交付中' : '创建 Jianying draft'}
              </button>
            </div>
            <ul className="workflow-summary">
              <li><b>{assetCount.total}</b><span>素材</span></li>
              <li><b>{assetCount.ready}</b><span>分析完成</span></li>
              <li><b>{assetCount.failed}</b><span>分析失败</span></li>
              <li><b>{storyboard?.shots.length ?? 0}</b><span>镜头</span></li>
              <li><b>{timeline?.clips.length ?? 0}</b><span>片段</span></li>
              <li><b>{preview ? preview.qualityReport.checks.length : 0}</b><span>preview 检查</span></li>
            </ul>
          </section>

          <section className="storyboard-view">
          {storyboard ? (
            <>
              <div className="storyboard-heading">
                <div>
                  <span className="eyebrow">storyboard v{storyboard.versionNumber} · 9:16 · English</span>
                  <h1>{storyboard.title}</h1>
                </div>
                <p>{storyboard.summary}</p>
                {storyboard.uncoveredBeatIds.length > 0 && <p className="storyboard-error">有 {storyboard.uncoveredBeatIds.length} 个信息点缺少可用素材，未被硬插入时间线。</p>}
              </div>
              <div className="shot-grid">
                {storyboard.shots.map((shot) => (
                  <article className="shot-card" key={shot.orderIndex}>
                    <div className={`shot-image shot-${String(shot.orderIndex).padStart(2, '0')}`}>
                      <span>{String(shot.orderIndex).padStart(2, '0')}</span>
                      <time>{formatEvidenceTime(shot.durationMs)}</time>
                    </div>
                    <div className="shot-copy">
                      <strong>{shot.purpose}</strong>
                      <p>
                        {assets.find((asset) => asset.id === shot.assetId)?.name ?? '已验证素材'} <span>{formatEvidenceTime(shot.sourceStartMs)} - {formatEvidenceTime(shot.sourceEndMs)}</span>
                      </p>
                      <small>{shot.matchLevel === 'direct' ? '直接匹配' : '语境匹配'} · {shot.reason}</small>
                      <em>{shot.onScreenText}</em>
                    </div>
                    <button onClick={() => { onSetActiveView('chat'); setInput(`调整第 ${shot.orderIndex} 个镜头：`) }}>让 Agent 调整</button>
                  </article>
                ))}
              </div>
            </>
          ) : (
            <div className="empty-storyboard">
              <span className="eyebrow">EVIDENCE-BOUND STORYBOARD</span>
              <h1>先告诉 Agent 要做什么</h1>
              <p>例如：用这些素材制作一条 30 秒英文产品宣传视频，突出工厂实力、产品质量和交付能力。</p>
              <textarea
                className="brief-input"
                value={storyboardBrief}
                onChange={(event) => onStoryboardBriefChange(event.target.value)}
                placeholder="描述视频目标、时长、语言、受众和重点信息"
                rows={5}
              />
              {storyboardError && <p className="storyboard-error">{storyboardError}</p>}
              <button className="primary-button" onClick={() => onGenerateStoryboard()} disabled={isGeneratingStoryboard || !storyboardBrief.trim()}>
                {isGeneratingStoryboard ? '正在生成' : '基于该需求生成故事板'}
              </button>
            </div>
          )}
          </section>

          {timeline && timeline.textTracks.length > 0 && (
            <section className="plan-card text-track-card artifact-card">
              <div className="plan-heading">
                <span>文本轨 · Agent 设计</span>
                <small>{timeline.textTracks.reduce((count, track) => count + track.cues.length, 0)} 个 cue</small>
              </div>
              <ul>
                {timeline.textTracks.flatMap((track) => track.cues.map((cue) => (
                  <li key={cue.id}>
                    <span className={cue.jianyingCompatibility === 'verified' ? 'text-compatible' : 'text-local'}>{cue.jianyingCompatibility === 'verified' ? 'Jianying 可交付' : '仅本地 preview'}</span>
                    <b>{cue.text}</b>
                    <small>
                      {formatEvidenceTime(cue.startMs)} - {formatEvidenceTime(cue.endMs)} · {cue.templateId ? `预设 ${cue.templateId} · ` : ''}{cue.style.fontKey}{cue.entrance ? ` · 入场 ${cue.entrance.templateId}` : ''}{cue.exit ? ` · 出场 ${cue.exit.templateId}` : ''}
                    </small>
                  </li>
                )))}
              </ul>
            </section>
          )}

          {preview && (
            <section className="preview-card artifact-card">
              <span className="eyebrow">LOCAL LOW-RES PREVIEW</span>
              <video controls src={`${convertFileSrc(preview.previewPath)}?v=${previewNonce}`} />
              {preview.qualityReport.checks.length > 0 && (
                <div className="quality-checks">
                  {preview.qualityReport.checks.map((check, index) => (
                    <p key={`${check.category}-${index}`} className={check.severity}>
                      {check.message}{check.shotIndices.length > 0 ? ` 镜头：${check.shotIndices.join('、')}` : ''}
                    </p>
                  ))}
                </div>
              )}
            </section>
          )}

          <AgentAuditPanel tasks={agentTasks} logs={operationLogs} timelineVersions={timelineVersions} />
        </div>
      )}
    </section>
  )
}
