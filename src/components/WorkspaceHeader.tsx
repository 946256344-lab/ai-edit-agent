// 当前项目/任务的顶栏摘要和主工作区切换器，不拥有导航状态。
import type { WorkspaceView } from './workspace-types'

export type WorkspaceHeaderModel = {
  projectName: string
  sessionTitle: string
  storeReady: boolean
  view: WorkspaceView
  assetCount: number
  shotCount: number
  hasStoryboard: boolean
  timelineLabel: string
}

type WorkspaceHeaderProps = {
  model: WorkspaceHeaderModel
  selectView: (view: WorkspaceView) => void
}

export function WorkspaceHeader({ model, selectView }: WorkspaceHeaderProps) {
  return (
    <>
      <header className="topbar">
        <div className="crumbs">{model.projectName} <span>/</span> {model.sessionTitle}</div>
        <div className="top-actions">
          <span className="saved">{model.storeReady ? 'local project' : '演示模式'}</span>
          {model.hasStoryboard && model.view !== 'artifacts' && (
            <button className="outline-button" onClick={() => selectView('artifacts')}>查看成果</button>
          )}
        </div>
      </header>
      <div className="mode-tabs">
        <button className={model.view === 'chat' ? 'selected' : ''} onClick={() => selectView('chat')}>Agent</button>
        <button className={model.view === 'assets' ? 'selected' : ''} onClick={() => selectView('assets')}>
          素材 <span>{model.assetCount}</span>
        </button>
        <button className={model.view === 'artifacts' ? 'selected' : ''} onClick={() => selectView('artifacts')}>
          成果 <span>{model.shotCount}</span>
        </button>
        <div className="timeline-state">{model.timelineLabel}</div>
      </div>
    </>
  )
}
