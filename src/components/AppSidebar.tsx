// 项目侧栏：项目内素材入口、剪辑会话与设置；操作仍交给应用 controller。
import { useEffect, useRef, useState } from 'react'
import type { StoredProject } from '../lib/local-store'
import type { AnalysisAmbientStatus } from '../lib/asset-analysis'
import type { EditingSessionView, WorkspaceView } from './workspace-types'
import { ProjectSettingsModal } from './ProjectSettingsModal'
import { NameEditDialog } from './NameEditDialog'
import { WorkspaceIcon } from './WorkspaceIcon'
import { useI18n } from '../lib/i18n'

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
  analysisStatus: AnalysisAmbientStatus
  analysisHint: string
  covers: Record<string, string>
  artworkNotice: string | null
}
export type AppSidebarActions = {
  createSession: () => void
  createProject: () => void
  selectProject: (projectId: string) => void
  selectSession: (sessionId: string) => void
  deleteSession: (sessionId: string) => void
  renameProject: (projectId: string, name: string) => Promise<void>
  deleteProject: (projectId: string) => void
  renameSession: (sessionId: string, title: string) => Promise<void>
  openProvider: () => void
  openAssets: () => void
}
export function AppSidebar({ model, actions }: { model: AppSidebarModel; actions: AppSidebarActions }) {
  const sidebar = useRef<HTMLElement>(null)
  const { t, locale, setLocale } = useI18n()
  const copy = t.sidebar
  const [settingsProjectId, setSettingsProjectId] = useState<string | null>(null)
  const [renameTarget, setRenameTarget] = useState<{ kind: 'project' | 'session'; id: string; name: string } | null>(null)
  const settingsProject = model.projects.find((project) => project.id === settingsProjectId)
  useEffect(() => {
    function closeOutsideMenus(event: PointerEvent) {
      const target = event.target as Node
      sidebar.current?.querySelectorAll<HTMLDetailsElement>('details[open]').forEach((menu) => {
        if (!menu.contains(target)) menu.open = false
      })
    }
    document.addEventListener('pointerdown', closeOutsideMenus)
    return () => document.removeEventListener('pointerdown', closeOutsideMenus)
  }, [])
  return (
    <aside ref={sidebar} className="sidebar project-sidebar">
      <span className="assembly-wordmark" data-tauri-drag-region>FellowCut</span>
      <div className="sidebar-project">
        <span className="sidebar-label">{copy.currentProject}</span>
        <details className="project-switcher" onKeyDown={(event) => {
          if (event.key === 'Escape') { event.currentTarget.open = false; event.currentTarget.querySelector('summary')?.focus() }
        }}>
          <summary className="project-selector" aria-label={copy.selectProject} title={model.activeProjectName ?? copy.selectProject}>
            <span className="project-symbol">{Object.values(model.covers).find(Boolean) ? <img src={Object.values(model.covers).find(Boolean)} alt="" /> : <WorkspaceIcon name="folder" />}</span>
            <strong>{model.activeProjectName ?? copy.selectProject}</strong><WorkspaceIcon name="chevron" />
          </summary>
          <div className="project-popover">
            <span className="sidebar-label">{copy.myProjects}</span>
            {model.projects.map((project) => <div className="project-option" key={project.id}>
              <button className={project.id === model.activeProjectId ? 'selected' : ''} onClick={(event) => { actions.selectProject(project.id); event.currentTarget.closest('.project-switcher')?.removeAttribute('open') }}>{project.name}</button>
              <details className="item-actions">
                <summary aria-label={copy.editItem(project.name)} title={copy.projectActions}>•••</summary>
                <div className="item-menu">
                  <button onClick={() => setRenameTarget({ kind: 'project', id: project.id, name: project.name })}>{t.common.rename}</button>
                  <button onClick={() => setSettingsProjectId(project.id)}>{copy.projectSettings}</button>
                  <button className="danger" onClick={() => actions.deleteProject(project.id)}>{copy.deleteProject}</button>
                </div>
              </details>
            </div>)}
            <button onClick={(event) => { actions.createProject(); event.currentTarget.closest('details')?.removeAttribute('open') }}><WorkspaceIcon name="plus" />{copy.newProject}</button>
          </div>
        </details>
        <button className={`sidebar-library ${model.view === 'assets' ? 'selected' : ''}`} aria-label={model.analysisStatus === 'analyzing' ? copy.libraryAnalyzing(model.assetCount) : model.analysisStatus === 'attention' ? copy.libraryAttention(model.assetCount) : copy.libraryIdle(model.assetCount)} aria-pressed={model.view === 'assets'} title={model.analysisStatus === 'idle' ? undefined : model.analysisHint} onClick={actions.openAssets}>
          <WorkspaceIcon name="library" /><span>{copy.library}</span><small>{model.assetCount}</small>
          {model.analysisStatus !== 'idle' && <i className={`sidebar-library__status sidebar-library__status--${model.analysisStatus}`} aria-hidden="true" />}
        </button>
        <button className="new-edit" onClick={actions.createSession} title={copy.newSessionTitle}><WorkspaceIcon name="plus" /><span>{copy.newEdit}</span></button>
      </div>
      <nav className="sidebar-sessions" aria-label={copy.sessions}>
        <span className="sidebar-label">{copy.sessions}</span>
        {!model.sessions.length && <p className="switcher-empty">{copy.sessionsEmpty}</p>}
        {model.sessions.map((session) => <div className={`session-row ${session.id === model.activeSessionId && model.view !== 'assets' ? 'selected' : ''}`} key={session.id}>
          <button className="session-select" aria-current={session.id === model.activeSessionId && model.view !== 'assets' ? 'page' : undefined} title={session.title} data-initial={session.title.trim().charAt(0) || copy.sessionInitial} onClick={() => actions.selectSession(session.id)}>
            <span className="session-copy"><strong>{session.title}</strong><small>{session.state === 'working' ? copy.editing : session.updated}</small></span>
          </button>
          <details className="item-actions session-actions">
            <summary aria-label={copy.editItem(session.title)} title={copy.sessionActions}>•••</summary>
            <div className="item-menu">
              <button onClick={() => setRenameTarget({ kind: 'session', id: session.id, name: session.title })}>{t.common.rename}</button>
              <button className="danger" onClick={() => actions.deleteSession(session.id)}>{copy.deleteSession}</button>
            </div>
          </details>
        </div>)}
        {model.artworkNotice && <p className="switcher-empty">{model.artworkNotice}</p>}
      </nav>
      <div className="project-sidebar-footer">
        <button title={copy.modelSettingsTitle(model.providerLabel)} aria-label={copy.modelSettings} onClick={actions.openProvider}><WorkspaceIcon name="model" /><span>{copy.modelSettings}</span></button>
        <button title={copy.projectSettings} aria-label={copy.projectSettings} onClick={() => setSettingsProjectId(model.activeProjectId)}><WorkspaceIcon name="settings" /><span>{copy.projectSettings}</span></button>
        <button title={t.language.switchTitle} aria-label={`${t.language.label}: ${t.language.switchTo}`} onClick={() => setLocale(locale === 'en' ? 'zh-CN' : 'en')}><WorkspaceIcon name="language" /><span>{t.language.switchTo}</span></button>
        <span className="local-status"><i className={`connection-dot ${model.storeState}`} /><span>{model.storeState === 'ready' ? t.common.localWorkspace : t.common.localDisconnected}</span></span>
      </div>
      <ProjectSettingsModal open={Boolean(settingsProjectId)} projectId={settingsProjectId} projectName={settingsProject?.name ?? null} onClose={() => setSettingsProjectId(null)} />
      <NameEditDialog
        open={Boolean(renameTarget)}
        label={renameTarget?.kind === 'session' ? copy.kindSession : copy.kindProject}
        initialValue={renameTarget?.name ?? ''}
        onClose={() => setRenameTarget(null)}
        onSave={(value) => renameTarget?.kind === 'project'
          ? actions.renameProject(renameTarget.id, value)
          : actions.renameSession(renameTarget?.id ?? '', value)}
      />
    </aside>
  )
}
