// 导入后的分析弹窗：真实计数、估算时间以及后台/取消操作。
import { useLayoutEffect, useRef } from 'react'
import type { useAssetAnalysisController } from '../hooks/useAssetAnalysisController'
import { analysisPendingCount } from '../lib/asset-analysis'
import { AssetAnalysisProgress } from './AssetAnalysisProgress'

export function AssetAnalysisModal({ controller }: { controller: ReturnType<typeof useAssetAnalysisController> }) {
  const { model, actions } = controller
  const dialog = useRef<HTMLDialogElement>(null)
  useLayoutEffect(() => {
    const element = dialog.current
    if (model.open) element?.showModal()
    return () => element?.close()
  }, [model.open])
  if (!model.open) return null
  const pending = analysisPendingCount(model.progress) > 0
  const seconds = model.remainingSeconds
  return <dialog ref={dialog} className="settings-dialog asset-analysis-dialog" aria-labelledby="analysis-title" onCancel={event => { event.preventDefault(); actions.background() }}>
    <span className="panel-kicker">素材准备</span>
    <h2 id="analysis-title">{pending ? '正在分析素材' : model.progress.cancelled ? '分析已取消' : model.progress.failed ? '部分素材分析失败' : '素材分析完成'}</h2>
    <p>{pending ? '正在识别画面与内容，分析完成后即可开始剪辑。' : '已完成的分析结果已保留，可在素材库查看。'}</p>
    {pending && <p className="asset-analysis-eta">预计剩余时间：{seconds === null ? '正在估算…' : seconds < 60 ? `约 ${seconds} 秒` : `约 ${Math.ceil(seconds / 60)} 分钟`}</p>}
    <AssetAnalysisProgress progress={model.progress} retrying={false} notice={model.notice} />
    <footer className="asset-dialog-actions">
      {pending && <button className="asset-dialog-secondary" disabled={model.busy} onClick={actions.cancelBatch}>取消分析</button>}
      {model.progress.cancelled > 0 && <button disabled={model.busy} onClick={actions.resumeBatch}>继续分析</button>}
      <button className="import-button" autoFocus onClick={actions.background}>{pending ? '后台分析' : '完成'}</button>
    </footer>
  </dialog>
}
