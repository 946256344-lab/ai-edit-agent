// 新建项目表单：只编辑草稿，关闭或取消不产生项目。
import { useLayoutEffect, useRef } from 'react'
import type { useProjectCreationController } from '../hooks/useProjectCreationController'
import { WorkspaceIcon } from './WorkspaceIcon'

export function ProjectCreationModal({ controller }: { controller: ReturnType<typeof useProjectCreationController> }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const { model, actions } = controller
  useLayoutEffect(() => {
    const element = dialog.current
    if (model.isOpen) element?.showModal()
    return () => element?.close()
  }, [model.isOpen])
  if (!model.isOpen) return null
  return <dialog ref={dialog} className="settings-dialog project-create-dialog" aria-labelledby="project-create-title" onCancel={(event) => { event.preventDefault(); actions.close() }}>
    <form onSubmit={(event) => { event.preventDefault(); void actions.submit() }}>
      <header><span className="eyebrow">NEW PROJECT</span><h2 id="project-create-title">开始一个新项目</h2><p>给创作起个名字，选好这次要使用的素材。</p></header>
      <fieldset disabled={model.saving}>
        <label className="project-name-field"><span>项目名称</span><input autoFocus required value={model.name} onChange={(event) => actions.setName(event.target.value)} placeholder="例如：秋日旅行短片" /></label>
        <div className="library-choice-heading"><span>子素材库 <small>已选 {model.selectedIds.length} / {model.libraries.length}</small></span><div><button type="button" onClick={actions.selectAll}>全选</button><button type="button" onClick={actions.clear}>取消全选</button></div></div>
        <p className="library-choice-hint">全局共享，默认全选。仅使用勾选库中的素材，不复制源文件。</p>
        <div className="library-choices">
          {model.loading ? <p>正在读取素材库…</p> : model.libraries.length === 0 ? <p>还没有子素材库，创建后可导入素材文件夹。</p> : model.libraries.map((library) => <label key={library.id} className="library-choice"><input type="checkbox" checked={model.selectedIds.includes(library.id)} onChange={() => actions.toggle(library.id)} /><WorkspaceIcon name="folder" /><span>{library.name}</span><small>{library.assetCount} 个素材</small></label>)}
        </div>
      </fieldset>
      {model.error && <p role="alert" className="project-create-error">{model.error}</p>}
      <footer><button type="button" className="outline-button" disabled={model.saving} onClick={actions.close}>取消</button><button type="submit" className="primary-button" disabled={model.loading || model.saving || !model.name.trim() || Boolean(model.error)}>{model.saving ? '创建中…' : '创建项目'}</button></footer>
    </form>
  </dialog>
}
