// 素材详情：原片预览、源片段播放与真实视觉内容；OCR 按需展开，不修改素材或时间线。
import { useRef, useState, type MouseEvent } from 'react'
import { convertFileSrc } from '@tauri-apps/api/core'
import type { AssetEvidence } from '../../lib/local-store'
import './asset-evidence.css'
import { messages, useI18n } from '../../lib/i18n'
import type { Messages } from '../../lib/i18n'

function formatTimeMs(timeMs: number | null) {
  if (timeMs === null) return messages().assetDetail.wholeAsset
  const seconds = Math.max(0, Math.floor(timeMs / 1000))
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
}

function evidenceLabel(item: AssetEvidence['visualEvidence'][number], t: Messages) {
  return [
    item.narrativeRole ?? '',
    item.caption ?? '',
    ...item.subjects, item.scene ?? '', ...item.actions, ...item.products,
    item.shotType ? t.assetDetail.shotType(item.shotType) : '',
    item.cameraMotion ? t.assetDetail.cameraMotion(item.cameraMotion) : '',
  ].filter(Boolean).join(' · ') || t.assetDetail.noVisualDescription
}

export function AssetEvidenceInspector({ evidence, onClose }: { evidence: AssetEvidence; onClose: () => void }) {
  const player = useRef<HTMLVideoElement>(null)
  const { t } = useI18n()
  const copy = t.assetDetail
  const range = useRef<{ startMs: number; endMs: number } | null>(null)
  const [selectedSegment, setSelectedSegment] = useState<string | null>(null)
  const [mediaReady, setMediaReady] = useState(false)
  const [mediaError, setMediaError] = useState(false)
  const segments = evidence.segments ?? []
  const mediaUrl = convertFileSrc(evidence.mediaPath)
  const isVideo = evidence.kind === 'video'

  function seekTo(timeMs: number) {
    const video = player.current
    if (!video) return
    video.currentTime = Math.max(0, timeMs) / 1000
  }

  function playSegment(segment: NonNullable<AssetEvidence['segments']>[number] | null) {
    const startMs = segment?.usableStartMs ?? segment?.startMs ?? 0
    const endMs = segment?.usableEndMs ?? segment?.endMs ?? 0
    range.current = segment ? { startMs, endMs } : null
    setSelectedSegment(segment?.id ?? null)
    const video = player.current!
    video.currentTime = startMs / 1000
    void video.play().catch(() => setMediaError(true))
  }

  return (
    <section className="asset-evidence-card asset-detail" aria-label={copy.aria}>
      <header className="asset-detail__header">
        <div><span className="panel-kicker">{copy.kicker}</span><h3 title={evidence.displayName}>{evidence.displayName}</h3></div>
        <button className="asset-detail__close" onClick={onClose} aria-label={copy.closeAria}>{t.common.close}</button>
      </header>

      <div className="asset-detail__media">
        {isVideo && <video ref={player} src={mediaUrl} controls playsInline preload="metadata" aria-label={copy.videoAria}
          onLoadedMetadata={() => setMediaReady(true)} onError={() => setMediaError(true)}
          onPlay={() => {
            const video = player.current!
            const clip = range.current
            if (clip && (video.currentTime < clip.startMs / 1000 || video.currentTime >= clip.endMs / 1000)) video.currentTime = clip.startMs / 1000
          }}
          onTimeUpdate={() => {
            const video = player.current!
            if (range.current && !video.paused && video.currentTime >= range.current.endMs / 1000) {
              video.pause()
              video.currentTime = range.current.endMs / 1000
            }
          }} />}
        {evidence.kind === 'audio' && <audio src={mediaUrl} controls preload="metadata" aria-label={copy.audioAria} onError={() => setMediaError(true)} />}
        {evidence.kind === 'image' && <img src={mediaUrl} alt={evidence.displayName} onError={() => setMediaError(true)} />}
      </div>
      {mediaError && <p className="asset-detail__error" role="status">{copy.mediaError}</p>}
      <p className="asset-detail__summary">
        {evidence.durationMs !== null && <span>{formatTimeMs(evidence.durationMs)}</span>}
        <span>{segments.length > 0 ? copy.segmentCount(segments.length) : copy.keyframeCount(evidence.keyframes.length)}</span>
        <span>{evidence.analysisStatus === 'failed' ? copy.basicFailed : evidence.kind === 'audio' || evidence.kind === 'other' ? (evidence.analysisStatus === 'ready' ? copy.analysisDone : copy.waitingBasic) : copy.visualStatus[evidence.visualAnalysisStatus]}</span>
      </p>
      {evidence.visualAnalysisNote && <p className="asset-detail__note">{evidence.visualAnalysisNote}</p>}

      {segments.length > 0 && <section className="asset-detail__section" aria-label={copy.segments}>
        <div className="asset-detail__section-heading"><h4>{copy.segments}</h4>{isVideo && <button disabled={!mediaReady || mediaError} onClick={() => playSegment(null)}>{copy.playFull}</button>}</div>
        {segments.map((segment, index) => <article className={`asset-detail__segment ${selectedSegment === segment.id ? 'is-selected' : ''}`} key={segment.id}>
          <button className="asset-detail__segment-play" disabled={!isVideo || !mediaReady || mediaError} aria-pressed={selectedSegment === segment.id} aria-label={copy.playSegmentAria(index + 1, formatTimeMs(segment.usableStartMs ?? segment.startMs), formatTimeMs(segment.usableEndMs ?? segment.endMs))} onClick={() => playSegment(segment)}>
            {segment.frames.length > 0 && <img src={convertFileSrc(segment.frames[0].imagePath)} alt={copy.segmentFrameAlt(index + 1)} loading="lazy" />}
            <span>
              <strong>{copy.segment(index + 1)}</strong>
              <small>{formatTimeMs(segment.startMs)} – {formatTimeMs(segment.endMs)}</small>
              {segment.usableStartMs != null && segment.usableEndMs != null
                && (segment.usableStartMs !== segment.startMs || segment.usableEndMs !== segment.endMs)
                && <small>{copy.usable(formatTimeMs(segment.usableStartMs), formatTimeMs(segment.usableEndMs))}</small>}
              {segment.motionTailSettled === false && <small>{copy.tailUnsettled}</small>}
              {segment.motionUncertain && <small>{copy.boundaryUncertain}</small>}
            </span>
            {isVideo && <span className="asset-detail__play-label">{copy.play}</span>}
          </button>
          <MotionEnergyChart segment={segment} onSeek={isVideo && mediaReady && !mediaError ? seekTo : undefined} />
          <p>{segment.visualEvidence ? evidenceLabel(segment.visualEvidence, t) : copy.noSegmentDescription}</p>
        </article>)}
      </section>}

      {segments.length === 0 && evidence.keyframes.length > 0 && <section className="asset-detail__section"><h4>{copy.keyframes}</h4><div className="asset-detail__frames">
        {evidence.keyframes.map(frame => <figure key={frame.imagePath}><img src={convertFileSrc(frame.imagePath)} alt={copy.keyframeAlt(formatTimeMs(frame.timeMs))} loading="lazy" /><figcaption>{formatTimeMs(frame.timeMs)}</figcaption></figure>)}
      </div></section>}

      {evidence.visualEvidence.length > 0 && <section className="asset-detail__section" aria-label={copy.visualAnalysis}>
        <h4>{copy.visualAnalysis}</h4>
        {evidence.visualEvidence.map((item, index) => <p className="asset-detail__visual" key={`${item.timeMs}-${index}`}><span>{formatTimeMs(item.timeMs)}</span>{evidenceLabel(item, t)}</p>)}
      </section>}

      {evidence.ocrEvidence.length > 0 && <details className="asset-detail__ocr"><summary>{copy.ocr} <span>{copy.ocrCount(evidence.ocrEvidence.length)}</span></summary>
        {evidence.ocrEvidence.map(item => <p key={`${item.timeMs}-${item.text}`}><span>{formatTimeMs(item.timeMs)}</span>{item.text}</p>)}
      </details>}
    </section>
  )
}

type AssetSegment = NonNullable<AssetEvidence['segments']>[number]

function MotionEnergyChart({
  segment,
  onSeek,
}: {
  segment: AssetSegment
  onSeek?: (timeMs: number) => void
}) {
  const copy = useI18n().t.assetDetail
  const samples = segment.motionEnergy ?? []
  if (samples.length < 2) {
    return <p className="asset-detail__motion-empty">{copy.motionEmpty}</p>
  }
  const width = 320
  const height = 52
  const pad = 3
  const span = Math.max(1, segment.endMs - segment.startMs)
  const peak = Math.max(...samples.map(sample => sample.energy), 0.001)
  const xAt = (timeMs: number) => pad + ((timeMs - segment.startMs) / span) * (width - pad * 2)
  const yAt = (energy: number) => height - pad - (energy / peak) * (height - pad * 2)
  const line = samples.map(sample => `${xAt(sample.timeMs).toFixed(1)},${yAt(sample.energy).toFixed(1)}`).join(' ')
  const area = `${xAt(samples[0].timeMs).toFixed(1)},${height - pad} ${line} ${xAt(samples[samples.length - 1].timeMs).toFixed(1)},${height - pad}`
  const usableStart = segment.usableStartMs ?? segment.startMs
  const usableEnd = segment.usableEndMs ?? segment.endMs
  const usableX = xAt(usableStart)
  const usableW = Math.max(0, xAt(usableEnd) - usableX)
  const trimmed = usableStart !== segment.startMs || usableEnd !== segment.endMs

  function handlePointer(event: MouseEvent<SVGSVGElement>) {
    if (!onSeek) return
    const box = event.currentTarget.getBoundingClientRect()
    const ratio = box.width <= 0 ? 0 : (event.clientX - box.left) / box.width
    onSeek(segment.startMs + Math.round(ratio * span))
  }

  return (
    <figure className="asset-detail__motion">
      <figcaption>{copy.motionCaption(trimmed)}</figcaption>
      <svg
        className={onSeek ? 'is-seekable' : undefined}
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label={copy.motionAria(formatTimeMs(segment.startMs), formatTimeMs(segment.endMs))}
        onClick={onSeek ? handlePointer : undefined}
      >
        {trimmed && <rect className="asset-detail__motion-usable" x={usableX} y={pad} width={usableW} height={height - pad * 2} rx="2" />}
        <polygon className="asset-detail__motion-fill" points={area} />
        <polyline className="asset-detail__motion-line" fill="none" points={line} />
      </svg>
    </figure>
  )
}
