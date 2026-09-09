// 成果工作区：默认只展示 Preview、状态与剪映交付；技术细节与手动兜底放在「查看详情」。
import { useState } from 'react'
import { convertFileSrc } from '@tauri-apps/api/core'
import { AgentAuditPanel } from './AgentAuditPanel'
import type {
  PreviewResult,
  StoryboardVersion,
  StoredAgentTask,
  StoredOperationLog,
  TimelineVersion,
} from '../lib/local-store'
import type { AssetPageCounts, StoryboardAsset } from './workspace-types'

export type ArtifactsWorkspaceModel = {
  assetCounts: AssetPageCounts
  assets: StoryboardAsset[]
  storyboard: StoryboardVersion | null
  storyboardBrief: string
  storyboardError: string | null
  timeline: TimelineVersion | null
  preview: PreviewResult | null
  previewNonce: number
  deliveryStatus: string
  jianyingNotice: string | null
  jianyingNoticeTone: 'info' | 'error'
  tasks: StoredAgentTask[]
  operationLogs: StoredOperationLog[]
  timelineVersions: TimelineVersion[]
  busy: {
    generatingStoryboard: boolean
    creatingTimeline: boolean
    renderingPreview: boolean
    creatingJianyingDraft: boolean
  }
}

export type ArtifactsWorkspaceActions = {
  setStoryboardBrief: (value: string) => void
  generateStoryboard: () => void
  createTimeline: () => void
  renderPreview: () => void
  createJianyingDraft: () => void
  adjustShot: (orderIndex: number) => void
  continueAdjust: () => void
}

type ArtifactsWorkspaceProps = {
  model: ArtifactsWorkspaceModel
  actions: ArtifactsWorkspaceActions
}

function formatEvidenceTime(timeMs: number | null) {
  if (timeMs === null) return '图片'
  const seconds = Math.floor(timeMs / 1000)
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
}

function formatDurationLabel(durationMs: number | null | undefined) {
  if (!durationMs || durationMs <= 0) return null
  const seconds = Math.max(1, Math.round(durationMs / 1000))
  return `${seconds} 秒`
}

function timelineDurationMs(timeline: TimelineVersion | null) {
  if (!timeline?.clips.length) return null
  return timeline.clips.reduce((max, clip) => Math.max(max, clip.timelineEndMs), 0)
}

function resultSummary(model: ArtifactsWorkspaceModel) {
  const shotDuration = model.storyboard?.shots.reduce((sum, shot) => sum + shot.durationMs, 0)
  const parts = [
    formatDurationLabel(timelineDurationMs(model.timeline) ?? shotDuration),
    '9:16',
    model.deliveryStatus,
  ].filter(Boolean)
  return parts.join(' · ')
}

export function ArtifactsWorkspace({ model, actions }: ArtifactsWorkspaceProps) {
  const { storyboard, timeline, preview, busy } = model
  const [detailsOpen, setDetailsOpen] = useState(false)

  return (
    <section className="conversation-workspace conversation-workspace--artifacts">
      <div className="artifact-stream">
        <section className="workflow-card artifact-workflow artifact-result">
          <div className="workflow-card-header">
            <div>
              <span className="panel-kicker">当前成果</span>
              <strong>{preview ? '预览已就绪' : storyboard ? '剪辑进行中' : '还没有成片'}</strong>
            </div>
            <small>{resultSummary(model)}</small>
          </div>

          {preview ? (
            <div className="artifact-preview-stage">
              <video controls src={`${convertFileSrc(preview.previewPath)}?v=${model.previewNonce}`} />
            </div>
          ) : (
            <div className="artifact-empty-preview">
              <p>
                {storyboard
                  ? '成片还在准备预览。也可以回到对话继续调整，或在下方详情里手动推进。'
                  : '先在 Agent 里用一句话说你想剪成什么，结果会显示在这里。'}
              </p>
            </div>
          )}

          <div className="workflow-actions artifact-primary-actions">
            <button
              className="primary-button"
              onClick={actions.createJianyingDraft}
              disabled={!timeline || busy.creatingJianyingDraft}
              title={!timeline ? '需要先有剪辑结果才能交付剪映' : undefined}
            >
              {busy.creatingJianyingDraft ? '交付中…' : '生成剪映草稿'}
            </button>
            <button className="outline-button" onClick={actions.continueAdjust}>
              继续调整
            </button>
          </div>
          {model.jianyingNotice && (
            <p className={model.jianyingNoticeTone === 'error' ? 'storyboard-error' : 'jianying-notice'}>
              {model.jianyingNotice}
            </p>
          )}
          {!timeline && preview && (
            <p className="storyboard-error">当前预览缺少对应剪辑结果，请回到 Agent 重新生成，或在下方详情里创建时间线。</p>
          )}
        </section>

        <details
          className="artifact-details"
          open={detailsOpen}
          onToggle={(event) => setDetailsOpen(event.currentTarget.open)}
        >
          <summary>查看详情</summary>

          <section className="workflow-card artifact-workflow">
            <div className="workflow-card-header">
              <div>
                <span className="panel-kicker">手动兜底</span>
                <strong>需要时再推进内部步骤</strong>
              </div>
              <small>
                素材 {model.assetCounts.ready}/{model.assetCounts.total}
                {model.assetCounts.failed > 0 ? ` · 失败 ${model.assetCounts.failed}` : ''}
              </small>
            </div>
            <div className="workflow-actions">
              <button className="primary-button" onClick={actions.createTimeline} disabled={!storyboard || busy.creatingTimeline}>
                {busy.creatingTimeline ? '创建中' : timeline ? '新建时间线版本' : '创建时间线'}
              </button>
              <button className="outline-button" onClick={actions.renderPreview} disabled={!timeline || busy.renderingPreview}>
                {busy.renderingPreview ? '生成中' : preview ? '重新生成 preview' : '生成 preview'}
              </button>
            </div>
          </section>

          <section className="storyboard-view">
            {storyboard ? (
              <>
                <div className="storyboard-heading">
                  <div>
                    <span className="eyebrow">storyboard v{storyboard.versionNumber} · 9:16</span>
                    <h1>{storyboard.title}</h1>
                  </div>
                  <p>{storyboard.summary}</p>
                  {storyboard.uncoveredBeatIds.length > 0 && (
                    <p className="storyboard-error">
                      有 {storyboard.uncoveredBeatIds.length} 个信息点缺少可用素材，未被硬插入时间线。
                    </p>
                  )}
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
                          {model.assets.find((asset) => asset.id === shot.assetId)?.name ?? '已验证素材'}{' '}
                          <span>{formatEvidenceTime(shot.sourceStartMs)} - {formatEvidenceTime(shot.sourceEndMs)}</span>
                        </p>
                        <small>{shot.matchLevel === 'direct' ? '直接匹配' : '语境匹配'} · {shot.reason}</small>
                        <em>{shot.onScreenText}</em>
                      </div>
                      <button onClick={() => actions.adjustShot(shot.orderIndex)}>让 Agent 调整</button>
                    </article>
                  ))}
                </div>
              </>
            ) : (
              <div className="empty-storyboard">
                <span className="eyebrow">手动生成</span>
                <h1>也可以在这里直接写需求</h1>
                <p>优先仍建议回到 Agent 对话。这里保留手动入口，方便兜底。</p>
                <textarea
                  className="brief-input"
                  value={model.storyboardBrief}
                  onChange={(event) => actions.setStoryboardBrief(event.target.value)}
                  placeholder="描述视频目标、时长、语言、受众和重点信息"
                  rows={5}
                />
                {model.storyboardError && <p className="storyboard-error">{model.storyboardError}</p>}
                <button
                  className="primary-button"
                  onClick={actions.generateStoryboard}
                  disabled={busy.generatingStoryboard || !model.storyboardBrief.trim()}
                >
                  {busy.generatingStoryboard ? '正在生成' : '基于该需求生成故事板'}
                </button>
              </div>
            )}
          </section>

          {timeline && timeline.textTracks.length > 0 && (
            <section className="plan-card text-track-card artifact-card">
              <div className="plan-heading">
                <span>文本轨</span>
                <small>{timeline.textTracks.reduce((count, track) => count + track.cues.length, 0)} 个 cue</small>
              </div>
              <ul>
                {timeline.textTracks.flatMap((track) => track.cues.map((cue) => (
                  <li key={cue.id}>
                    <span className={cue.jianyingCompatibility === 'verified' ? 'text-compatible' : 'text-local'}>
                      {cue.jianyingCompatibility === 'verified' ? 'Jianying 可交付' : '仅本地 preview'}
                    </span>
                    <b>{cue.text}</b>
                    <small>
                      {formatEvidenceTime(cue.startMs)} - {formatEvidenceTime(cue.endMs)} ·{' '}
                      {cue.templateId ? `预设 ${cue.templateId} · ` : ''}{cue.style.fontKey}
                      {cue.entrance ? ` · 入场 ${cue.entrance.templateId}` : ''}{cue.exit ? ` · 出场 ${cue.exit.templateId}` : ''}
                    </small>
                  </li>
                )))}
              </ul>
            </section>
          )}

          {preview && preview.qualityReport.checks.length > 0 && (
            <section className="preview-card artifact-card">
              <span className="eyebrow">预览检查</span>
              <div className="quality-checks">
                {preview.qualityReport.checks.map((check, index) => (
                  <p key={`${check.category}-${index}`} className={check.severity}>
                    {check.message}{check.shotIndices.length > 0 ? ` 镜头：${check.shotIndices.join('、')}` : ''}
                  </p>
                ))}
              </div>
            </section>
          )}

          <AgentAuditPanel tasks={model.tasks} logs={model.operationLogs} timelineVersions={model.timelineVersions} />
        </details>
      </div>
    </section>
  )
}
