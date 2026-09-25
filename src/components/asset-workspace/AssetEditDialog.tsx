// 素材显示名编辑与移除确认；本地媒体及已有时间线记录保留。
import { useLayoutEffect, useRef } from 'react'
import type { AssetLibraryEditController } from '../../hooks/useAssetLibraryEditController'
import { useI18n } from '../../lib/i18n'

export function AssetEditDialog({ model, actions }: AssetLibraryEditController) {
  const dialog = useRef<HTMLDialogElement>(null)
  const { t } = useI18n()
  const copy = t.assets
  const open = Boolean(model.rename || model.removing)
  useLayoutEffect(() => {
    const element = dialog.current
    if (open) element?.showModal()
    return () => element?.close()
  }, [open])
  if (!open) return null
  return <dialog ref={dialog} className="settings-dialog asset-analysis-dialog" aria-labelledby="asset-edit-title" onCancel={event => { event.preventDefault(); if (!model.busy) actions.close() }}>
    <form onSubmit={event => { event.preventDefault(); actions.save() }}>
      <h2 id="asset-edit-title">{model.rename ? copy.renameTitle : copy.removeTitle(model.selectedIds.length)}</h2>
      {model.rename ? <><label htmlFor="asset-display-name">{copy.nameLabel}</label><input id="asset-display-name" className="asset-name-input" value={model.rename.name} onChange={event => actions.setName(event.target.value)} autoFocus disabled={model.busy} /><p>{copy.renameHint}</p></> : <p>{copy.removeBody}</p>}
      {model.notice && <p role="alert">{model.notice}</p>}
      <footer className="asset-dialog-actions"><button type="button" onClick={actions.close} disabled={model.busy}>{t.common.cancel}</button><button className="import-button" disabled={model.busy}>{model.busy ? copy.savingProgress : model.rename ? copy.saveName : copy.removeFromLibrary}</button></footer>
    </form>
  </dialog>
}
