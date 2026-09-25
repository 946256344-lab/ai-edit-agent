// 新建项目表单：只编辑草稿，关闭或取消不产生项目。
import { useLayoutEffect, useRef } from 'react'
import type { useProjectCreationController } from '../hooks/useProjectCreationController'
import { WorkspaceIcon } from './WorkspaceIcon'
import { useI18n } from '../lib/i18n'

export function ProjectCreationModal({ controller }: { controller: ReturnType<typeof useProjectCreationController> }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const { model, actions } = controller
  const { t } = useI18n()
  const copy = t.projectCreate
  useLayoutEffect(() => {
    const element = dialog.current
    if (model.isOpen) element?.showModal()
    return () => element?.close()
  }, [model.isOpen])
  if (!model.isOpen) return null
  return <dialog ref={dialog} className="settings-dialog project-create-dialog" aria-labelledby="project-create-title" onCancel={(event) => { event.preventDefault(); actions.close() }}>
    <form onSubmit={(event) => { event.preventDefault(); void actions.submit() }}>
      <header><span className="eyebrow">NEW PROJECT</span><h2 id="project-create-title">{copy.title}</h2><p>{copy.subtitle}</p></header>
      <fieldset disabled={model.saving}>
        <label className="project-name-field"><span>{copy.nameLabel}</span><input autoFocus required value={model.name} onChange={(event) => actions.setName(event.target.value)} placeholder={copy.namePlaceholder} /></label>
        <div className="library-choice-heading"><span>{copy.libraries} <small>{copy.selectedCount(model.selectedIds.length, model.libraries.length)}</small></span><div><button type="button" onClick={actions.selectAll}>{t.common.selectAll}</button><button type="button" onClick={actions.clear}>{t.common.clearSelection}</button></div></div>
        <p className="library-choice-hint">{copy.hint}</p>
        <div className="library-choices">
          {model.loading ? <p>{copy.loadingLibraries}</p> : model.libraries.length === 0 ? <p>{copy.noLibraries}</p> : model.libraries.map((library) => <label key={library.id} className="library-choice"><input type="checkbox" checked={model.selectedIds.includes(library.id)} onChange={() => actions.toggle(library.id)} /><WorkspaceIcon name="folder" /><span>{library.name}</span><small>{copy.assetCount(library.assetCount)}</small></label>)}
        </div>
      </fieldset>
      {model.error && <p role="alert" className="project-create-error">{model.error}</p>}
      <footer><button type="button" className="outline-button" disabled={model.saving} onClick={actions.close}>{t.common.cancel}</button><button type="submit" className="primary-button" disabled={model.loading || model.saving || !model.name.trim() || Boolean(model.error)}>{model.saving ? copy.creating : copy.create}</button></footer>
    </form>
  </dialog>
}
