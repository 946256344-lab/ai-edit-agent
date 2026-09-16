// 项目与会话维护 controller：负责名称编辑、项目确认删除和删除后的导航状态。
import type { Dispatch, MutableRefObject, SetStateAction } from 'react'
import type { EditingSessionView } from '../components/workspace-types'
import {
  deleteProject,
  renameEditingSession,
  renameProject,
} from '../lib/local-store'
import type { StoredProject } from '../lib/local-store'

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
      `确定删除项目「${project?.name ?? '该项目'}」？\n\n将永久删除项目内的素材索引、分析结果、剪辑会话和本地预览。原始媒体与已创建的剪映草稿不会删除。`,
    )
    if (!confirmed) return
    try {
      await deleteProject(projectId, true)
    } catch {
      window.alert('删除项目失败，请稍后重试。')
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
