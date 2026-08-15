// 成果工作区：集中展示 storyboard、timeline、preview 和交付审计，不直接调用后端命令。
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

export function ArtifactsWorkspace({ model, actions }: ArtifactsWorkspaceProps) {
  const { storyboard, timeline, preview, busy } = model

  return (
    <section className="conversation-workspace conversation-workspace--artifacts">
      <div className="artifact-stream">
        <section className="workflow-card artifact-workflow">
          <div className="workflow-card-header">
            <div>
              <span className="panel-kicker">当前成果</span>
              <strong>storyboard → timeline → preview → Jianying draft</strong>
            </div>
            <small>{model.deliveryStatus}</small>
          </div>
          <div className="workflow-actions">
            <button className="primary-button" onClick={actions.createTimeline} disabled={!storyboard || busy.creatingTimeline}>
              {busy.creatingTimeline ? '创建中' : timeline ? '新建时间线版本' : '创建时间线'}
            </button>
            <button className="outline-button" onClick={actions.renderPreview} disabled={!timeline || busy.renderingPreview}>
              {busy.renderingPreview ? '生成中' : preview ? '重新生成 preview' : '生成 preview'}
            </button>
            <button className="outline-button" onClick={actions.createJianyingDraft} disabled={!timeline || busy.creatingJianyingDraft}>
              {busy.creatingJianyingDraft ? '交付中' : '创建 Jianying draft'}
            </button>
          </div>
          <ul className="workflow-summary">
            <li><b>{model.assetCounts.total}</b><span>素材</span></li>
            <li><b>{model.assetCounts.ready}</b><span>分析完成</span></li>
            <li><b>{model.assetCounts.failed}</b><span>分析失败</span></li>
            <li><b>{storyboard?.shots.length ?? 0}</b><span>镜头</span></li>
            <li><b>{timeline?.clips.length ?? 0}</b><span>片段</span></li>
            <li><b>{preview?.qualityReport.checks.length ?? 0}</b><span>preview 检查</span></li>
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
                {storyboard.uncoveredBeatIds.length > 0 && (
                  <p className="storyboard-error">有 {storyboard.uncoveredBeatIds.length} 个信息点缺少可用素材，未被硬插入时间线。</p>
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
              <span className="eyebrow">EVIDENCE-BOUND STORYBOARD</span>
              <h1>先告诉 Agent 要做什么</h1>
              <p>例如：用这些素材制作一条 30 秒英文产品宣传视频，突出工厂实力、产品质量和交付能力。</p>
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
              <span>文本轨 · Agent 设计</span>
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

        {preview && (
          <section className="preview-card artifact-card">
            <span className="eyebrow">LOCAL LOW-RES PREVIEW</span>
            <video controls src={`${convertFileSrc(preview.previewPath)}?v=${model.previewNonce}`} />
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

        <AgentAuditPanel tasks={model.tasks} logs={model.operationLogs} timelineVersions={model.timelineVersions} />
      </div>
    </section>
  )
}
