// 只读展示所选素材的技术元数据、关键帧、OCR 和视觉证据。
import { convertFileSrc } from '@tauri-apps/api/core'
import type { AssetEvidence } from '../../lib/local-store'

function formatTimeMs(timeMs: number | null) {
  if (timeMs === null) return '图片'
  const seconds = Math.max(0, Math.floor(timeMs / 1000))
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
}

function visualStatusLabel(status: AssetEvidence['visualAnalysisStatus']) {
  switch (status) {
    case 'ready': return '视觉分析已完成。'
    case 'running': return '视觉分析正在运行。'
    case 'queued': return '视觉分析正在排队。'
    case 'failed': return '视觉分析未完成。'
    case 'skipped': return '视觉分析已跳过。'
  }
}

export function AssetEvidenceInspector({ evidence, onClose }: { evidence: AssetEvidence | null; onClose: () => void }) {
  if (!evidence) {
    return (
      <section className="asset-evidence-empty">
        <span className="eyebrow">INSPECTOR</span>
        <strong>选择一条素材查看证据</strong>
        <p>这里仅显示技术分析、关键帧、OCR 与视觉证据。</p>
      </section>
    )
  }

  return (
    <section className="asset-evidence-card">
      <div className="asset-evidence-card__head">
        <div>
          <span className="panel-kicker">画面证据</span>
          <strong>{evidence.displayName}</strong>
        </div>
        <button className="close-button" onClick={onClose} aria-label="关闭">x</button>
      </div>
      <p>{evidence.visualAnalysisNote ?? visualStatusLabel(evidence.visualAnalysisStatus)}</p>
      <div className="asset-evidence-grid">
        <article><b>{evidence.durationMs !== null ? formatTimeMs(evidence.durationMs) : '—'}</b><span>时长</span></article>
        <article><b>{evidence.keyframes.length}</b><span>关键帧</span></article>
        <article><b>{evidence.ocrEvidence.length}</b><span>OCR</span></article>
        <article><b>{evidence.visualEvidence.length}</b><span>视觉</span></article>
      </div>
      {evidence.keyframes.length > 0 && (
        <div className="asset-evidence-frames">
          {evidence.keyframes.map((frame) => (
            <figure key={frame.imagePath}>
              <img src={convertFileSrc(frame.imagePath)} alt="" />
              <figcaption>{formatTimeMs(frame.timeMs)}</figcaption>
            </figure>
          ))}
        </div>
      )}
      {evidence.ocrEvidence.length > 0 && (
        <div className="asset-evidence-section">
          <strong>OCR 文本</strong>
          {evidence.ocrEvidence.map((item) => <p key={`${item.timeMs}-${item.text}`}><span>{formatTimeMs(item.timeMs)}</span>{item.text}</p>)}
        </div>
      )}
      {evidence.visualEvidence.length > 0 && (
        <div className="asset-evidence-section">
          <strong>视觉标签</strong>
          {evidence.visualEvidence.map((item, index) => (
            <p key={`${item.timeMs}-${index}`}>
              <span>{formatTimeMs(item.timeMs)}</span>
              {[...item.subjects, item.scene ?? '', ...item.actions, ...item.products].filter(Boolean).join(' · ') || '未返回可用视觉标签'}
            </p>
          ))}
        </div>
      )}
    </section>
  )
}
