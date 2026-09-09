// 窄侧栏：项目/会话选择折叠在菜单内，保留设置入口。
import { useState } from 'react'
import type { StoredProject } from '../lib/local-store'
import type { EditingSessionView } from './workspace-types'
import { ProjectSettingsModal } from './ProjectSettingsModal'

export type AppSidebarModel = {
  projects: StoredProject[]
  activeProjectId: string | null
  activeProjectName: string | null
  sessions: EditingSessionView[]
  activeSessionId: string | null
  providerLabel: string
  storeState: 'browser' | 'ready' | 'unavailable'
}
export type AppSidebarActions = {
  createSession: () => void
  createProject: () => void
  selectProject: (projectId: string) => void
  selectSession: (sessionId: string) => void
  deleteSession: (sessionId: string) => void
  openProvider: () => void
}
export function AppSidebar({ model, actions }: { model: AppSidebarModel; actions: AppSidebarActions }) {
  const [projectSettingsOpen, setProjectSettingsOpen] = useState(false)
  return (
    <aside className="sidebar icon-rail">
      <span className="brand-mark" title="Assembly">A</span>
      <button className="rail-button" title="新建剪辑会话" aria-label="新建剪辑会话" onClick={actions.createSession}>＋</button>
      <details className="project-switcher">
        <summary className="rail-button" title="项目与会话" aria-label="项目与会话">▤</summary>
        <div className="project-popover">
          <div className="switcher-heading"><strong>项目与会话</strong><button className="text-button" onClick={actions.createProject}>新建项目</button></div>
          <label>当前项目<select value={model.activeProjectId ?? ''} onChange={(event) => actions.selectProject(event.target.value)}><option value="" disabled>选择项目</option>{model.projects.map((project) => <option key={project.id} value={project.id}>{project.name}</option>)}</select></label>
          <nav aria-label="剪辑会话">{model.sessions.map((session) => <div className="session-menu-row" key={session.id}><button className={session.id === model.activeSessionId ? 'selected' : ''} onClick={(event) => { actions.selectSession(session.id); event.currentTarget.closest('details')?.removeAttribute('open') }}><strong>{session.title}</strong><small>{session.preview}</small></button><button title="删除会话" aria-label={`删除${session.title}`} onClick={() => actions.deleteSession(session.id)}>×</button></div>)}</nav>
          <button className="outline-button" onClick={actions.createSession}>＋ 新建剪辑会话</button>
        </div>
      </details>
      <div className="rail-bottom"><button className="rail-button" title={`模型设置 · ${model.providerLabel}`} aria-label="模型设置" onClick={actions.openProvider}>✧</button><button className="rail-button" title="项目设置" aria-label="项目设置" onClick={() => setProjectSettingsOpen(true)}>⚙</button><span className={`connection-dot ${model.storeState}`} title={model.storeState === 'ready' ? '本地已连接' : '本地连接不可用'} /></div>
      <ProjectSettingsModal open={projectSettingsOpen} projectId={model.activeProjectId} projectName={model.activeProjectName} onClose={() => setProjectSettingsOpen(false)} />
    </aside>
  )
}
