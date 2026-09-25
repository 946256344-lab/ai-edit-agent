// 导入后的分析弹窗：真实计数、估算时间以及后台/取消操作。
import { useLayoutEffect, useRef } from 'react'
import type { useAssetAnalysisController } from '../hooks/useAssetAnalysisController'
import { analysisPendingCount } from '../lib/asset-analysis'
import { AssetAnalysisProgress } from './AssetAnalysisProgress'
import { useI18n } from '../lib/i18n'

export function AssetAnalysisModal({ controller }: { controller: ReturnType<typeof useAssetAnalysisController> }) {
  const { model, actions } = controller
  const dialog = useRef<HTMLDialogElement>(null)
  const { t } = useI18n()
  const copy = t.analysis
  useLayoutEffect(() => {
    const element = dialog.current
    if (model.open) element?.showModal()
    return () => element?.close()
  }, [model.open])
  if (!model.open) return null
  const pending = analysisPendingCount(model.progress) > 0
  const seconds = model.remainingSeconds
  return <dialog ref={dialog} className="settings-dialog asset-analysis-dialog" aria-labelledby="analysis-title" onCancel={event => { event.preventDefault(); actions.background() }}>
    <span className="panel-kicker">{t.analysisGate.kicker}</span>
    <h2 id="analysis-title">{pending ? copy.modalAnalyzing : model.progress.cancelled ? copy.modalCancelled : model.progress.failed ? copy.modalFailed : copy.modalDone}</h2>
    <p>{pending ? copy.modalPendingBody : copy.modalDoneBody}</p>
    {pending && <p className="asset-analysis-eta">{copy.eta(seconds === null ? copy.estimating : seconds < 60 ? copy.aboutSeconds(seconds) : copy.aboutMinutes(Math.ceil(seconds / 60)))}</p>}
    <AssetAnalysisProgress progress={model.progress} retrying={false} notice={model.notice} />
    <footer className="asset-dialog-actions">
      {pending && <button className="asset-dialog-secondary" disabled={model.busy} onClick={actions.cancelBatch}>{copy.cancel}</button>}
      {model.progress.cancelled > 0 && <button disabled={model.busy} onClick={actions.resumeBatch}>{copy.resume}</button>}
      <button className="import-button" autoFocus onClick={actions.background}>{pending ? copy.background : copy.done}</button>
    </footer>
  </dialog>
}
