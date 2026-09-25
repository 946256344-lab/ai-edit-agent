// 粗剪预览与候选面板：始终与对话并排，试选画面和已保存的整片预览分开呈现。
import { useEffect, useRef, useState } from 'react'
import { convertFileSrc } from '@tauri-apps/api/core'
import type { ArtifactWorkspaceController } from '../hooks/useArtifactWorkspaceController'
import type { ShotReplacementController } from '../hooks/useShotReplacementController'
import { WorkspaceIcon } from './WorkspaceIcon'
import { RoughCutPlayer } from './RoughCutPlayer'
import { useI18n } from '../lib/i18n'

type Props = {
  model: { artifact: ArtifactWorkspaceController['model']; replacement: ShotReplacementController['model']; agentBusy: boolean }
  actions: { artifact: ArtifactWorkspaceController['actions']; replacement: ShotReplacementController['actions'] }
}

export function RoughCutPreview({ model: { artifact, replacement, agentBusy }, actions }: Props) {
  const video = useRef<HTMLVideoElement>(null)
  const { t } = useI18n()
  const copy = t.preview
  const pendingDialog = useRef<HTMLDialogElement>(null)
  const strip = useRef<HTMLDivElement>(null)
  const [playhead, setPlayhead] = useState(0)
  // 镜头条默认收起，让出空间给画面；点镜头计数展开。
  const [showStrip, setShowStrip] = useState(false)
  const { shot, prepared, phase, recommendations, selectedId } = replacement
  const busy = phase === 'saving' || phase === 'rendering' || artifact.busy.renderingPreview || artifact.busy.delivering || agentBusy
  const clips = artifact.timeline?.clips ?? []
  const currentClip = clips.find((clip) => playhead >= clip.timelineStartMs && playhead < clip.timelineEndMs) ?? clips[0]
  const stalePreview = artifact.preview && artifact.preview.timelineVersionId !== artifact.timeline?.id
  const selected = recommendations?.candidates.find((candidate) => candidate.candidateId === selectedId)
  // Rust 只给中文原因：可用窗短于原镜头即“时长不足”，否则为素材不可用；按界面语言重述。
  const shotMs = shot ? shot.timelineEndMs - shot.timelineStartMs : 0
  const unavailableText = (candidate: { unavailableReason: string | null; durationMs: number | null }) => candidate.unavailableReason
    ? (candidate.durationMs ?? 0) < shotMs
      ? t.backend.candidateTooShort((shotMs / 1000).toFixed(1))
      : t.backend.candidateUnavailable
    : null
  // 镜头条一屏只放 6 个：展开时播放到哪一镜，就把它滚到可见位置。
  useEffect(() => {
    strip.current?.querySelector<HTMLElement>('button.selected')?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
  }, [currentClip?.shotIndex, showStrip])
  useEffect(() => {
    if (replacement.pending) pendingDialog.current?.showModal()
    else pendingDialog.current?.close()
  }, [replacement.pending])
  return (
    <section className="rough-preview" aria-label={copy.aria}>
      <header className="preview-heading">
        <h2>{shot ? copy.replaceTitle(String(shot.shotIndex).padStart(2, '0')) : copy.title}</h2>
        <span className="format-label">{shot ? copy.durationKept(((shot.timelineEndMs - shot.timelineStartMs) / 1000).toFixed(1)) : artifact.deliveryStatus}</span>
      </header>
      {shot ? (
        <div className="replacement-body">
          <div className="candidate-preview">
            <div className="candidate-video">
              {prepared ? <video key={prepared.previewPath} src={convertFileSrc(prepared.previewPath)} controls autoPlay muted loop playsInline /> : <span>{phase === 'preparing' ? copy.preparingCandidate : copy.pickCandidate}</span>}
            </div>
            <div><span className="eyebrow">{copy.trialEyebrow}</span><h3>{selected?.displayName ?? copy.trialFallbackTitle}</h3><p>{recommendations?.beatPurpose}</p><small>{copy.trialHint}</small></div>
          </div>
          <div className="candidate-heading"><strong>{copy.recommended}</strong><span>{recommendations ? copy.candidateCount(recommendations.candidates.length) : t.common.loading}</span></div>
          {recommendations && !recommendations.saved && <div className="preview-empty"><p>{copy.notSaved}</p><button className="outline-button" onClick={actions.replacement.generate}>{copy.generateRecommendations}</button></div>}
          {recommendations?.saved && !recommendations.candidates.length && <p className="workspace-notice">{copy.noCandidates}</p>}
          <div className="candidate-grid">
            {recommendations?.candidates.map((candidate, index) => (
              <button key={candidate.candidateId} className={`candidate-card ${candidate.candidateId === selectedId ? 'selected' : ''}`} disabled={busy || phase === 'preparing' || candidate.current || Boolean(candidate.unavailableReason)} onClick={() => actions.replacement.select(candidate.candidateId)} aria-pressed={candidate.candidateId === selectedId} title={unavailableText(candidate) ?? candidate.displayName}>
                <div className="candidate-image">{candidate.thumbnailPath ? <img src={convertFileSrc(candidate.thumbnailPath)} alt="" loading="lazy" decoding="async" /> : <span>{copy.noThumbnail}</span>}<b>{String(index + 1).padStart(2, '0')}</b><small>{candidate.current ? copy.currentShot : candidate.usedInTimeline ? copy.usedElsewhere : `${((candidate.durationMs ?? 0) / 1000).toFixed(1)}s`}</small></div>
                <span>{candidate.displayName}</span><small>{copy.sourceRange((candidate.sourceStartMs / 1000).toFixed(1), (candidate.sourceEndMs / 1000).toFixed(1))}</small>{candidate.unavailableReason && <small>{unavailableText(candidate)}</small>}
              </button>
            ))}
          </div>
        </div>
      ) : (
        <>
          <div className={`rough-player ${artifact.preview ? 'has-video' : ''}`}>
            {artifact.preview ? <RoughCutPlayer key={`${artifact.preview.previewPath}:${artifact.previewNonce}`} videoRef={video} src={convertFileSrc(artifact.preview.previewPath)} onTimeChange={setPlayhead} segments={stalePreview ? [] : clips.map((clip) => ({ startMs: clip.timelineStartMs, endMs: clip.timelineEndMs }))} /> : <div className="preview-empty"><span className="empty-play">▷</span><h3>{agentBusy ? copy.makingFirstCut : copy.firstCutFromChat}</h3><p>{copy.emptyLine1}<br />{copy.emptyLine2}</p>{artifact.storyboard && <button className="outline-button" disabled={busy || artifact.busy.creatingTimeline} onClick={artifact.timeline ? actions.artifact.renderPreview : actions.artifact.createTimeline}>{artifact.timeline ? copy.renderPreview : copy.createTimeline}</button>}</div>}
          </div>
          <div className="preview-tools">
            {currentClip
              ? <button className="shot-toggle" aria-expanded={showStrip} aria-controls="preview-shot-strip" title={`${copy.currentShotLine(String(currentClip.shotIndex).padStart(2, '0'), ((currentClip.timelineEndMs - currentClip.timelineStartMs) / 1000).toFixed(1))} · ${showStrip ? copy.hideShots : copy.showShots}`} onClick={() => setShowStrip((open) => !open)}><WorkspaceIcon name="film" /><span>{String(currentClip.shotIndex).padStart(2, '0')}/{String(clips.length).padStart(2, '0')}</span></button>
              : <span className="current-shot">{copy.waitingShot}</span>}
            <div><button className="outline-button" disabled={!currentClip || busy || (currentClip.clipKind ?? 'source') !== 'source'} onClick={() => { if (currentClip) actions.replacement.open(currentClip) }}>{copy.replaceCurrent}</button><button className="text-button icon-button" title={copy.undo} aria-label={copy.undo} disabled={!replacement.canUndo || busy} onClick={actions.replacement.undo}><WorkspaceIcon name="undo" /></button><button className="text-button icon-button" title={copy.redo} aria-label={copy.redo} disabled={!replacement.canRedo || busy} onClick={actions.replacement.redo}><WorkspaceIcon name="redo" /></button>{artifact.timeline && <button className={`text-button icon-button ${stalePreview ? 'is-stale' : ''}`} title={copy.updatePreview} aria-label={copy.updatePreview} disabled={busy} onClick={actions.artifact.renderPreview}><WorkspaceIcon name="refresh" /></button>}</div>
          </div>
          {showStrip && clips.length > 0 && <div className="shot-strip" id="preview-shot-strip" ref={strip} aria-label={copy.stripAria}>
            {clips.map((clip) => {
              const image = artifact.shotImages[clip.shotIndex]
              return <button key={clip.shotIndex} className={clip.shotIndex === currentClip?.shotIndex ? 'selected' : ''} onClick={() => { setPlayhead(clip.timelineStartMs); if (video.current) video.current.currentTime = clip.timelineStartMs / 1000 }} aria-label={copy.locateShot(clip.shotIndex)} aria-pressed={clip.shotIndex === currentClip?.shotIndex} title={image?.displayName ?? copy.shot(clip.shotIndex)}>
                <span className="shot-thumbnail">{image ? <img src={convertFileSrc(image.imagePath)} alt="" loading="lazy" /> : <WorkspaceIcon name="film" />}</span>
                <span className="shot-caption">{String(clip.shotIndex).padStart(2, '0')}</span>
              </button>
            })}
          </div>}
        </>
      )}
      {(replacement.notice || stalePreview || artifact.deliveryNotice || artifact.thumbnailNotice) && <div className="workspace-notice" role="status">{replacement.notice && <p>{replacement.notice}</p>}{stalePreview && !replacement.notice && <p>{copy.stalePreview}</p>}{artifact.deliveryNotice && <p>{artifact.deliveryNotice}</p>}{artifact.thumbnailNotice && <p>{artifact.thumbnailNotice}</p>}</div>}
      {shot && <footer className="preview-footer"><span>{copy.footerReplacing}</span><div><button className="outline-button" disabled={phase === 'saving'} onClick={actions.replacement.cancel}>{t.common.cancel}</button><button className="primary-button" disabled={!prepared || phase !== 'idle' || busy} onClick={actions.replacement.save}>{phase === 'saving' ? t.common.savingEllipsis : copy.saveChanges}</button></div></footer>}
      <dialog ref={pendingDialog} aria-labelledby="pending-title" className="pending-dialog" onCancel={actions.replacement.keepEditing}><h3 id="pending-title">{copy.pendingTitle}</h3><p>{copy.pendingBody}</p><button className="primary-button" disabled={!prepared || phase !== 'idle'} onClick={actions.replacement.saveAndContinue}>{copy.saveAndContinue}</button><button className="outline-button" onClick={actions.replacement.discardAndContinue}>{copy.discardAndContinue}</button><button className="text-button" onClick={actions.replacement.keepEditing}>{copy.keepEditing}</button></dialog>
    </section>
  )
}
