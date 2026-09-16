// 素材显示名编辑与移除确认；本地媒体及已有时间线记录保留。
import { useLayoutEffect, useRef } from 'react'
import type { AssetLibraryEditController } from '../../hooks/useAssetLibraryEditController'

export function AssetEditDialog({ model, actions }: AssetLibraryEditController) {
  const dialog = useRef<HTMLDialogElement>(null)
  const open = Boolean(model.rename || model.removing)
  useLayoutEffect(() => {
    const element = dialog.current
    if (open) element?.showModal()
    return () => element?.close()
  }, [open])
  if (!open) return null
  return <dialog ref={dialog} className="settings-dialog asset-analysis-dialog" aria-labelledby="asset-edit-title" onCancel={event => { event.preventDefault(); if (!model.busy) actions.close() }}>
    <form onSubmit={event => { event.preventDefault(); actions.save() }}>
      <h2 id="asset-edit-title">{model.rename ? '重命名素材' : `移除 ${model.selectedIds.length} 个素材？`}</h2>
      {model.rename ? <><label htmlFor="asset-display-name">素材名称</label><input id="asset-display-name" className="asset-name-input" value={model.rename.name} onChange={event => actions.setName(event.target.value)} autoFocus disabled={model.busy} /><p>仅修改素材库中的名称，本地文件名保持不变。</p></> : <p>从共享素材库移除，使用这些素材的项目将不再从库中选择它们。本地原文件和已有时间线记录保留，未完成的分析会取消。</p>}
      {model.notice && <p role="alert">{model.notice}</p>}
      <footer className="asset-dialog-actions"><button type="button" onClick={actions.close} disabled={model.busy}>取消</button><button className="import-button" disabled={model.busy}>{model.busy ? '正在保存…' : model.rename ? '保存名称' : '移出素材库'}</button></footer>
    </form>
  </dialog>
}
