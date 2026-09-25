// 项目与会话维护 controller：负责名称编辑、项目确认删除和删除后的导航状态。
import type { Dispatch, MutableRefObject, SetStateAction } from 'react'
import type { EditingSessionView } from '../components/workspace-types'
import {
  deleteProject,
  renameEditingSession,
  renameProject,
} from '../lib/local-store'
import type { StoredProject } from '../lib/local-store'
import { messages } from '../lib/i18n'

type NavigationEditControllerOptions = {
  projects: StoredProject[]
  activeProjectId: string | null
  activeProjectRef: MutableRefObject<string | null>
  setProjects: Dispatch<SetStateAction<StoredProject[]>>
  setSessions: Dispatch<SetStateAction<EditingSessionView[]>>
  selectProject: (projectId: string) => Promise<void>
  clearActiveProject: () => void
}

export function useNavigationEditController(options: NavigationEditControllerOptions) {
  async function renameProjectWorkspace(projectId: string, name: string) {
    const project = await renameProject(projectId, name)
    options.setProjects((current) => current.map((candidate) => candidate.id === projectId ? project : candidate))
  }

  async function renameEditingSessionWorkspace(sessionId: string, title: string) {
    if (!options.activeProjectId) return
    await renameEditingSession(options.activeProjectId, sessionId, title)
    options.setSessions((current) => current.map((session) => session.id === sessionId
      ? { ...session, title }
      : session))
  }

  async function deleteProjectWorkspace(projectId: string) {
    const project = options.projects.find((candidate) => candidate.id === projectId)
    const confirmed = window.confirm(
      messages().app.deleteProjectConfirm(project?.name ?? messages().app.projectFallback),
    )
    if (!confirmed) return
    try {
      await deleteProject(projectId, true)
    } catch {
      window.alert(messages().app.deleteProjectFailed)
      return
    }
    const remaining = options.projects.filter((candidate) => candidate.id !== projectId)
    options.setProjects(remaining)
    if (options.activeProjectRef.current !== projectId) return
    options.clearActiveProject()
    if (remaining[0]) await options.selectProject(remaining[0].id)
  }

  return {
    actions: {
      renameProject: renameProjectWorkspace,
      renameSession: renameEditingSessionWorkspace,
      deleteProject: deleteProjectWorkspace,
    },
  }
}
