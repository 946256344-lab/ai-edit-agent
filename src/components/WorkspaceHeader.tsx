// 工作区顶栏：项目面包屑与返回粗剪入口，素材库归属左侧项目。
import type { WorkspaceView } from './workspace-types'
import { WorkspaceIcon } from './WorkspaceIcon'
export type WorkspaceHeaderModel = {
  projectName: string
  sessionTitle: string
  storeReady: boolean
  view: WorkspaceView
}
export function WorkspaceHeader({ model, selectView }: { model: WorkspaceHeaderModel; selectView: (view: WorkspaceView) => void }) {
  return <>
    <header className="topbar">
      <div className="crumbs" title={`${model.projectName} / ${model.sessionTitle}`}>{model.projectName}<span>/</span><strong>{model.sessionTitle}</strong></div>
      <span className={`saved ${model.storeReady ? 'is-ready' : ''}`}>{model.storeReady ? '本地工作区' : '本地未连接'}</span>
    </header>
    <nav className="workspace-tabs" aria-label="工作区">
      <button className={model.view !== 'assets' ? 'selected' : ''} aria-pressed={model.view !== 'assets'} onClick={() => selectView('chat')}><WorkspaceIcon name="film" />粗剪工作台</button>
      {model.view === 'assets' && <span className="workspace-location">素材库</span>}
    </nav>
  </>
}
