// 剪辑前确认：未分析完时询问是否只用已分析素材，不自动开始。
import { useLayoutEffect, useRef } from 'react'
import type { AssetAnalysisProgress } from '../lib/local-store'
import './asset-analysis.css'
import { useI18n } from '../lib/i18n'

type Props = {
  open: boolean
  progress: AssetAnalysisProgress | null
  importing: boolean
  onUseReady: () => void
  onCancel: () => void
  onOpenLibrary: () => void
}

export function AnalysisIncompleteDialog({ open, progress, importing, onUseReady, onCancel, onOpenLibrary }: Props) {
  const dialog = useRef<HTMLDialogElement>(null)
  const { t } = useI18n()
  const copy = t.analysisGate
  useLayoutEffect(() => {
    const element = dialog.current
    if (open) element?.showModal()
    return () => element?.close()
  }, [open])
  if (!open) return null
  const incomplete = progress ? Math.max(0, progress.total - progress.ready) : 0
  const canUseReady = (progress?.readyVideo ?? 0) > 0
  return (
    <dialog ref={dialog} className="settings-dialog asset-analysis-dialog" aria-labelledby="analysis-gate-title" onCancel={(event) => { event.preventDefault(); onCancel() }}>
      <span className="panel-kicker">{copy.kicker}</span>
      <h2 id="analysis-gate-title">{importing ? copy.importingTitle : copy.incompleteTitle}</h2>
      {importing ? (
        <p>{copy.importingBody}</p>
      ) : (
        <>
          <p>{copy.incompleteBody(incomplete)}</p>
          {progress && <p>{copy.readyBody(progress.ready, progress.readyVideo)}</p>}
          {!canUseReady && <p>{copy.noReady}</p>}
        </>
      )}
      <footer className="asset-dialog-actions">
        <button type="button" className="asset-dialog-secondary" onClick={onOpenLibrary}>{copy.openLibrary}</button>
        <button type="button" autoFocus={importing || !canUseReady} onClick={onCancel}>{t.common.cancel}</button>
        {!importing && <button type="button" className="import-button" autoFocus={canUseReady} disabled={!canUseReady} onClick={onUseReady}>{copy.useReady}</button>}
      </footer>
    </dialog>
  )
}
