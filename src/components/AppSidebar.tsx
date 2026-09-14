// 项目侧栏：项目内素材入口、剪辑会话与设置；操作仍交给应用 controller。
import { useState } from 'react'
import type { StoredProject } from '../lib/local-store'
import type { EditingSessionView, WorkspaceView } from './workspace-types'
import { ProjectSettingsModal } from './ProjectSettingsModal'
import { WorkspaceIcon } from './WorkspaceIcon'

export type AppSidebarModel = {
  projects: StoredProject[]
  activeProjectId: string | null
  activeProjectName: string | null
  sessions: EditingSessionView[]
  activeSessionId: string | null
  providerLabel: string
  storeState: 'browser' | 'ready' | 'unavailable'
  view: WorkspaceView
  assetCount: number
  covers: Record<string, string>
  artworkNotice: string | null
}
export type AppSidebarActions = {
  createSession: () => void
  createProject: () => void
  selectProject: (projectId: string) => void
  selectSession: (sessionId: string) => void
  deleteSession: (sessionId: string) => void
  openProvider: () => void
  openAssets: () => void
}
export function AppSidebar({ model, actions }: { model: AppSidebarModel; actions: AppSidebarActions }) {
  const [projectSettingsOpen, setProjectSettingsOpen] = useState(false)
  return (
    <aside className="sidebar project-sidebar">
      <span className="assembly-wordmark">Assembly</span>
      <div className="sidebar-project">
        <span className="sidebar-label">当前项目</span>
        <details className="project-switcher" onKeyDown={(event) => {
          if (event.key === 'Escape') { event.currentTarget.open = false; event.currentTarget.querySelector('summary')?.focus() }
        }}>
          <summary className="project-selector" aria-label="选择项目" title={model.activeProjectName ?? '选择项目'}>
            <span className="project-symbol">{Object.values(model.covers).find(Boolean) ? <img src={Object.values(model.covers).find(Boolean)} alt="" /> : <WorkspaceIcon name="folder" />}</span>
            <strong>{model.activeProjectName ?? '选择项目'}</strong><WorkspaceIcon name="chevron" />
          </summary>
          <div className="project-popover">
            <span className="sidebar-label">我的项目</span>
            {model.projects.map((project) => <button key={project.id} className={project.id === model.activeProjectId ? 'selected' : ''} onClick={(event) => { actions.selectProject(project.id); event.currentTarget.closest('details')?.removeAttribute('open') }}>{project.name}</button>)}
            <button onClick={(event) => { actions.createProject(); event.currentTarget.closest('details')?.removeAttribute('open') }}><WorkspaceIcon name="plus" />新建项目</button>
          </div>
        </details>
        <button className={`sidebar-library ${model.view === 'assets' ? 'selected' : ''}`} aria-label="素材库" aria-pressed={model.view === 'assets'} onClick={actions.openAssets}>
          <WorkspaceIcon name="library" /><span>素材库</span><small>{model.assetCount}</small>
        </button>
        <button className="new-edit" onClick={actions.createSession} title="新建剪辑会话"><WorkspaceIcon name="plus" /><span>新建剪辑</span></button>
      </div>
      <nav className="sidebar-sessions" aria-label="剪辑会话">
        <span className="sidebar-label">剪辑会话</span>
        {!model.sessions.length && <p className="switcher-empty">从一个新的剪辑开始。</p>}
        {model.sessions.map((session) => <div className={`session-row ${session.id === model.activeSessionId && model.view !== 'assets' ? 'selected' : ''}`} key={session.id}>
          <button className="session-select" aria-current={session.id === model.activeSessionId && model.view !== 'assets' ? 'page' : undefined} title={session.title} data-initial={session.title.trim().charAt(0) || '剪'} onClick={() => actions.selectSession(session.id)}>
            <span className="session-copy"><strong>{session.title}</strong><small>{session.state === 'working' ? '正在剪辑…' : session.updated}</small></span>
          </button>
          <button className="session-delete" title="删除会话" aria-label={`删除${session.title}`} onClick={() => actions.deleteSession(session.id)}><WorkspaceIcon name="close" /></button>
        </div>)}
        {model.artworkNotice && <p className="switcher-empty">{model.artworkNotice}</p>}
      </nav>
      <div className="project-sidebar-footer">
        <button title={`模型设置 · ${model.providerLabel}`} aria-label="模型设置" onClick={actions.openProvider}><WorkspaceIcon name="model" /><span>模型设置</span></button>
        <button title="项目设置" aria-label="项目设置" onClick={() => setProjectSettingsOpen(true)}><WorkspaceIcon name="settings" /><span>项目设置</span></button>
        <span className="local-status"><i className={`connection-dot ${model.storeState}`} /><span>{model.storeState === 'ready' ? '本地工作区' : '本地未连接'}</span></span>
      </div>
      <ProjectSettingsModal open={projectSettingsOpen} projectId={model.activeProjectId} projectName={model.activeProjectName} onClose={() => setProjectSettingsOpen(false)} />
    </aside>
  )
}
