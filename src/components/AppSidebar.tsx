// 应用侧栏：切换 local project、剪辑任务与顶层工作区，不拥有数据加载逻辑。
import type { StoredProject } from '../lib/local-store'
import type { EditingSessionView } from './workspace-types'

export type AppSidebarModel = {
  projects: StoredProject[]
  activeProjectId: string | null
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
  openProvider: () => void
}

type AppSidebarProps = {
  model: AppSidebarModel
  actions: AppSidebarActions
}

export function AppSidebar({ model, actions }: AppSidebarProps) {
  return (
    <aside className="sidebar">
      <div className="brand">
        <span className="brand-mark">A</span><span>ASSEMBLY</span><small>VIDEO AGENT</small>
      </div>
      <button className="new-chat" onClick={actions.createSession}>+ 新建剪辑会话</button>
      <div className="side-label side-label-row">
        <span>项目</span>
        <button className="add-project" onClick={actions.createProject} aria-label="新建项目">+</button>
      </div>
      <nav className="project-list" aria-label="本地项目">
        {model.projects.map((project) => (
          <button
            key={project.id}
            className={`project-card ${project.id === model.activeProjectId ? 'active' : ''}`}
            onClick={() => actions.selectProject(project.id)}
          >
            <span className="project-dot" />
            <span><strong>{project.name}</strong><small>本地项目</small></span>
          </button>
        ))}
        {!model.projects.length && <p className="empty-projects">新建会话即可开始本地项目。</p>}
      </nav>
      <div className="side-label side-label-row"><span>剪辑会话</span><span>{model.sessions.length}</span></div>
      <nav className="conversation-list" aria-label="剪辑会话">
        {model.sessions.map((session) => (
          <button
            key={session.id}
            className={`conversation ${session.id === model.activeSessionId ? 'active' : ''}`}
            onClick={() => actions.selectSession(session.id)}
          >
            <span className={`state-dot ${session.state}`} />
            <span><strong>{session.title}</strong><small>{session.preview}</small></span>
            <time>{session.updated}</time>
          </button>
        ))}
      </nav>
      <div className="sidebar-footer">
        <button onClick={actions.openProvider}><span className="provider-dot" /> {model.providerLabel}</button>
        <button><span className="gear">o</span> 项目设置</button>
        <span className={`store-state ${model.storeState}`}>
          {model.storeState === 'ready' ? '本地 SQLite 已就绪' : model.storeState === 'browser' ? '浏览器原型模式' : '本地存储不可用'}
        </span>
      </div>
    </aside>
  )
}
