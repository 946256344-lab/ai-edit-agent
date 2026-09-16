// 素材详情：原片预览、源片段播放与真实视觉内容；OCR 按需展开，不修改素材或时间线。
import { useRef, useState, type MouseEvent } from 'react'
import { convertFileSrc } from '@tauri-apps/api/core'
import type { AssetEvidence } from '../../lib/local-store'
import './asset-evidence.css'

function formatTimeMs(timeMs: number | null) {
  if (timeMs === null) return '整条素材'
  const seconds = Math.max(0, Math.floor(timeMs / 1000))
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
}

function visualStatusLabel(status: AssetEvidence['visualAnalysisStatus']) {
  switch (status) {
    case 'ready': return '分析完成'
    case 'running': return '分析中'
    case 'queued': return '等待分析'
    case 'failed': return '分析未完成'
    case 'skipped': return '已跳过视觉分析'
  }
}

function evidenceLabel(item: AssetEvidence['visualEvidence'][number]) {
  return [
    item.narrativeRole ?? '',
    item.caption ?? '',
    ...item.subjects, item.scene ?? '', ...item.actions, ...item.products,
    item.shotType ? `景别：${item.shotType}` : '',
    item.cameraMotion ? `运镜：${item.cameraMotion}` : '',
  ].filter(Boolean).join(' · ') || '暂无画面描述'
}

export function AssetEvidenceInspector({ evidence, onClose }: { evidence: AssetEvidence; onClose: () => void }) {
  const player = useRef<HTMLVideoElement>(null)
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
    <section className="asset-evidence-card asset-detail" aria-label="素材详情">
      <header className="asset-detail__header">
        <div><span className="panel-kicker">素材详情</span><h3 title={evidence.displayName}>{evidence.displayName}</h3></div>
        <button className="asset-detail__close" onClick={onClose} aria-label="关闭素材详情">关闭</button>
      </header>

      <div className="asset-detail__media">
        {isVideo && <video ref={player} src={mediaUrl} controls playsInline preload="metadata" aria-label="原始视频预览"
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
        {evidence.kind === 'audio' && <audio src={mediaUrl} controls preload="metadata" aria-label="原始音频预览" onError={() => setMediaError(true)} />}
        {evidence.kind === 'image' && <img src={mediaUrl} alt={evidence.displayName} onError={() => setMediaError(true)} />}
      </div>
      {mediaError && <p className="asset-detail__error" role="status">此素材当前无法在播放器中打开。</p>}
      <p className="asset-detail__summary">
        {evidence.durationMs !== null && <span>{formatTimeMs(evidence.durationMs)}</span>}
        <span>{segments.length > 0 ? `${segments.length} 个片段` : `${evidence.keyframes.length} 张关键帧`}</span>
        <span>{visualStatusLabel(evidence.visualAnalysisStatus)}</span>
      </p>
      {evidence.visualAnalysisNote && <p className="asset-detail__note">{evidence.visualAnalysisNote}</p>}

      {segments.length > 0 && <section className="asset-detail__section" aria-label="场景片段">
        <div className="asset-detail__section-heading"><h4>场景片段</h4>{isVideo && <button disabled={!mediaReady || mediaError} onClick={() => playSegment(null)}>播放完整视频</button>}</div>
        {segments.map((segment, index) => <article className={`asset-detail__segment ${selectedSegment === segment.id ? 'is-selected' : ''}`} key={segment.id}>
          <button className="asset-detail__segment-play" disabled={!isVideo || !mediaReady || mediaError} aria-pressed={selectedSegment === segment.id} aria-label={`播放片段 ${index + 1}，${formatTimeMs(segment.usableStartMs ?? segment.startMs)} 至 ${formatTimeMs(segment.usableEndMs ?? segment.endMs)}`} onClick={() => playSegment(segment)}>
            {segment.frames.length > 0 && <img src={convertFileSrc(segment.frames[0].imagePath)} alt={`片段 ${index + 1} 关键帧`} loading="lazy" />}
            <span>
              <strong>片段 {index + 1}</strong>
              <small>{formatTimeMs(segment.startMs)} – {formatTimeMs(segment.endMs)}</small>
              {segment.usableStartMs != null && segment.usableEndMs != null
                && (segment.usableStartMs !== segment.startMs || segment.usableEndMs !== segment.endMs)
                && <small>可用 {formatTimeMs(segment.usableStartMs)} – {formatTimeMs(segment.usableEndMs)}</small>}
              {segment.motionTailSettled === false && <small>结尾未收住</small>}
              {segment.motionUncertain && <small>边界不确定</small>}
            </span>
            {isVideo && <span className="asset-detail__play-label">播放</span>}
          </button>
          <MotionEnergyChart segment={segment} onSeek={isVideo && mediaReady && !mediaError ? seekTo : undefined} />
          <p>{segment.visualEvidence ? evidenceLabel(segment.visualEvidence) : '暂无片段画面描述'}</p>
        </article>)}
      </section>}

      {segments.length === 0 && evidence.keyframes.length > 0 && <section className="asset-detail__section"><h4>关键帧</h4><div className="asset-detail__frames">
        {evidence.keyframes.map(frame => <figure key={frame.imagePath}><img src={convertFileSrc(frame.imagePath)} alt={`${formatTimeMs(frame.timeMs)} 关键帧`} loading="lazy" /><figcaption>{formatTimeMs(frame.timeMs)}</figcaption></figure>)}
      </div></section>}

      {evidence.visualEvidence.length > 0 && <section className="asset-detail__section" aria-label="素材视觉分析">
        <h4>素材视觉分析</h4>
        {evidence.visualEvidence.map((item, index) => <p className="asset-detail__visual" key={`${item.timeMs}-${index}`}><span>{formatTimeMs(item.timeMs)}</span>{evidenceLabel(item)}</p>)}
      </section>}

      {evidence.ocrEvidence.length > 0 && <details className="asset-detail__ocr"><summary>画面文字识别 <span>{evidence.ocrEvidence.length} 条</span></summary>
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
  const samples = segment.motionEnergy ?? []
  if (samples.length < 2) {
    return <p className="asset-detail__motion-empty">尚无运动能量曲线</p>
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
      <figcaption>运动能量{trimmed ? ' · 色带为可用窗' : ''}</figcaption>
      <svg
        className={onSeek ? 'is-seekable' : undefined}
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label={`片段运动能量曲线，从 ${formatTimeMs(segment.startMs)} 到 ${formatTimeMs(segment.endMs)}`}
        onClick={onSeek ? handlePointer : undefined}
      >
        {trimmed && <rect className="asset-detail__motion-usable" x={usableX} y={pad} width={usableW} height={height - pad * 2} rx="2" />}
        <polygon className="asset-detail__motion-fill" points={area} />
        <polyline className="asset-detail__motion-line" fill="none" points={line} />
      </svg>
    </figure>
  )
}
