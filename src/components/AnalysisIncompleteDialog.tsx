// 剪辑前确认：未分析完时询问是否只用已分析素材，不自动开始。
import { useLayoutEffect, useRef } from 'react'
import type { AssetAnalysisProgress } from '../lib/local-store'
import './asset-analysis.css'

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
      <span className="panel-kicker">素材准备</span>
      <h2 id="analysis-gate-title">{importing ? '正在导入素材' : '素材尚未全部分析完成'}</h2>
      {importing ? (
        <p>导入完成后才能开始剪辑。可取消这次发送，剪辑要求会留在输入框。</p>
      ) : (
        <>
          <p>现在还有 {incomplete} 个素材没有分析完成，是否只使用已分析的素材进行剪辑？</p>
          {progress && <p>已分析 {progress.ready} 个，其中 {progress.readyVideo} 条视频可用。</p>}
          {!canUseReady && <p>目前还没有可剪辑的已分析视频，请等待分析或去素材库查看。</p>}
        </>
      )}
      <footer className="asset-dialog-actions">
        <button type="button" className="asset-dialog-secondary" onClick={onOpenLibrary}>去素材库</button>
        <button type="button" autoFocus={importing || !canUseReady} onClick={onCancel}>取消</button>
        {!importing && <button type="button" className="import-button" autoFocus={canUseReady} disabled={!canUseReady} onClick={onUseReady}>只用已分析素材</button>}
      </footer>
    </dialog>
  )
}
