// 应用组合根：选择项目/会话，装配领域 controller 与并排对话、粗剪预览。
import { useEffect, useRef, useState } from 'react'
import './App.css'
import './light-workspace.css'
import { AgentWorkspace } from './components/AgentWorkspace'
import { PairedWorkspace } from './components/PairedWorkspace'
import { AssetAnalysisModal } from './components/AssetAnalysisModal'
import { AnalysisIncompleteDialog } from './components/AnalysisIncompleteDialog'
import { AppSidebar } from './components/AppSidebar'
import { RoughCutPreview } from './components/RoughCutPreview'
import { AssetManagementPanel } from './components/AssetManagementPanel'
import { ProviderSettingsModal } from './components/ProviderSettingsModal'
import { ReleaseReadinessBanner } from './components/ReleaseReadinessBanner'
import { EditorOutputPort } from './components/EditorOutputPort'
import { WorkspaceHeader } from './components/WorkspaceHeader'
import { useSessionArtworkController } from './hooks/useSessionArtworkController'
import { useComposerMediaController } from './hooks/useComposerMediaController'
import { useProjectCreationController } from './hooks/useProjectCreationController'
import { ProjectCreationModal } from './components/ProjectCreationModal'
import { useWindowController } from './hooks/useWindowController'
import type { ConversationMessage, EditingSessionView, WorkspaceView } from './components/workspace-types'
import { useAgentRunReconciliation } from './hooks/useAgentRunReconciliation'
import type { PendingAgentEdit } from './hooks/useAgentRunReconciliation'
import { useArtifactWorkspaceController } from './hooks/useArtifactWorkspaceController'
import { useShotReplacementController } from './hooks/useShotReplacementController'
import { useAssetWorkspaceController } from './hooks/useAssetWorkspaceController'
import { useAnalysisGateController } from './hooks/useAnalysisGateController'
import { useProviderController } from './hooks/useProviderController'
import { useNavigationEditController } from './hooks/useNavigationEditController'
import {
  createConversation as createStoredConversation,
  createEditingSession as createStoredEditingSession,
  createMessage as createStoredMessage,
  createProject as createStoredProject,
  cancelAgentEdit as cancelStoredAgentEdit,
  deleteEditingSession as deleteStoredEditingSession,
  initializeLocalStore,
  isDesktopRuntime,
  listAgentTasks,
  listEditingSessions,
  listMessages,
  listProjects,
  resolveConversationTask,
  setConversationStatus,
  submitConversationTurn,
} from './lib/local-store'
import type { AgentEditEvent, ConversationTurnResult, StoredAgentTask, StoredEditingSession, StoredMessage, StoredProject, TaskRouteResult } from './lib/local-store'
import { toMessage } from './lib/message'
import { analysisAmbientStatus, analysisSummary } from './lib/asset-analysis'

function toEditingSession(session: StoredEditingSession): EditingSessionView {
  return {
    id: session.id,
    conversationId: session.conversationId,
    title: session.title,
    preview: session.summary || session.brief || '暂无消息',
    brief: session.brief,
    updated: new Date(session.updatedAt).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' }),
    state: session.status === 'working' ? 'working' : session.status === 'review' ? 'review' : 'ready',
  }
}

function App() {
  const desktopRuntime = isDesktopRuntime()
  const projectCreation = useProjectCreationController(createProjectWorkspace)
  const windowControls = useWindowController(desktopRuntime)
  const [projects, setProjects] = useState<StoredProject[]>([])
  const [activeProjectId, setActiveProjectId] = useState<string | null>(null)
  const [editingSessions, setEditingSessions] = useState<EditingSessionView[]>([])
  const [activeEditingSessionId, setActiveEditingSessionId] = useState<string | null>(null)
  const [messages, setMessages] = useState<ConversationMessage[]>([])
  // 发送后、任务归属模型返回并落库前先显示的用户消息；落库后由真实消息替换。
  const [pendingUserMessage, setPendingUserMessage] = useState<ConversationMessage | null>(null)
  const [activeView, setActiveView] = useState<WorkspaceView>('chat')
  const [input, setInput] = useState('')
  const [isSending, setIsSending] = useState(false)
  const [composerNotice, setComposerNotice] = useState<string | null>(null)
  const cancelRequestedRef = useRef(false)
  const [routeStatusText, setRouteStatusText] = useState<string | null>(null)
  const [routeStatusDetail, setRouteStatusDetail] = useState<string | null>(null)
  const [routeStatusTone, setRouteStatusTone] = useState<'neutral' | 'info' | 'success' | 'warning'>('neutral')
  const [agentTasks, setAgentTasks] = useState<StoredAgentTask[]>([])
  const composerMedia = useComposerMediaController(activeEditingSessionId, agentTasks)
  const [storeState, setStoreState] = useState<'browser' | 'ready' | 'unavailable'>(desktopRuntime ? 'unavailable' : 'browser')
  const activeProjectRef = useRef<string | null>(null)
  const activeEditingSessionRef = useRef<string | null>(null)
  const activeProject = projects.find((project) => project.id === activeProjectId)
  const activeEditingSession = editingSessions.find((session) => session.id === activeEditingSessionId)
  const sessionArtwork = useSessionArtworkController(activeProjectId, editingSessions)
  const provider = useProviderController(desktopRuntime)
  const artifactWorkspace = useArtifactWorkspaceController({
    desktopRuntime,
    projectId: activeProjectId,
    sessionId: activeEditingSessionId,
    session: activeEditingSession,
    activeProjectRef,
    activeSessionRef: activeEditingSessionRef,
    setAgentTasks,
    appendAgentMessage: (conversationId, sessionId, content) => (
      appendStoredMessage(conversationId, sessionId, 'agent', content)
    ),
  })
  const agentReconciliation = useAgentRunReconciliation({
    desktopRuntime,
    projectId: activeProjectId,
    sessionId: activeEditingSessionId,
    conversationId: activeEditingSession?.conversationId,
    sessionState: activeEditingSession?.state,
    isSending,
    tasks: agentTasks,
    activeProjectRef,
    activeSessionRef: activeEditingSessionRef,
    setTasks: setAgentTasks,
    setIsSending,
    setComposerNotice,
    applyCompletion: applyAgentEditCompletion,
  })
  const assetWorkspace = useAssetWorkspaceController({
    desktopRuntime,
    storeReady: storeState === 'ready',
    projectId: activeProjectId,
    session: activeEditingSession,
    activeProjectRef,
    ensureEditingSession,
    appendAgentMessage: (conversationId, sessionId, content) => (
      appendStoredMessage(conversationId, sessionId, 'agent', content)
    ),
    refreshEditingSessions,
  })
  const analysisGate = useAnalysisGateController(activeProjectId, activeEditingSessionId, assetWorkspace.model.importing || assetWorkspace.model.retrying)
  const shotReplacement = useShotReplacementController({
    projectId: activeProjectId,
    sessionId: activeEditingSessionId,
    timeline: artifactWorkspace.timeline,
    activeProjectRef,
    activeSessionRef: activeEditingSessionRef,
    applyCommit: artifactWorkspace.applyStudioCommit,
  })
  const navigationEditing = useNavigationEditController({
    projects,
    activeProjectId,
    activeProjectRef,
    setProjects,
    setSessions: setEditingSessions,
    selectProject,
    clearActiveProject: () => {
      assetWorkspace.reset()
      artifactWorkspace.reset()
      activeProjectRef.current = null
      activeEditingSessionRef.current = null
      setActiveProjectId(null)
      setActiveEditingSessionId(null)
      setEditingSessions([])
      setMessages([])
      setAgentTasks([])
    },
  })

  async function applyAgentEditCompletion(pending: PendingAgentEdit, event?: AgentEditEvent) {
    const { projectId, sessionId } = pending
    const result = event?.result
    const isActiveScope = activeProjectRef.current === projectId
      && activeEditingSessionRef.current === sessionId
    if (isActiveScope && result) artifactWorkspace.applyAgentResult(result)
    const refreshedSessions = await refreshEditingSessions(projectId)
    if (activeProjectRef.current !== projectId || activeEditingSessionRef.current !== sessionId) return
    await selectEditingSession(projectId, sessionId, refreshedSessions)
  }

  useEffect(() => {
    if (!desktopRuntime) return
    void initializeLocalStore()
      .then(async () => {
        const storedProjects = await listProjects()
        setProjects(storedProjects)
        setStoreState('ready')
        if (storedProjects[0]) await selectProject(storedProjects[0].id)
      })
      .catch(() => setStoreState('unavailable'))
    // Local store bootstrap owns the initial project selection and must not rerun when
    // render-scoped controller objects are recreated.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [desktopRuntime])

  async function selectProject(projectId: string) {
    assetWorkspace.reset()
    artifactWorkspace.reset()
    activeProjectRef.current = projectId
    setActiveProjectId(projectId)
    const storedSessions = await listEditingSessions(projectId)
    if (activeProjectRef.current !== projectId) return
    const nextSessions = storedSessions.map(toEditingSession)
    setEditingSessions(nextSessions)
    if (nextSessions[0]) await selectEditingSession(projectId, nextSessions[0].id, nextSessions)
    else {
      setActiveEditingSessionId(null)
      activeEditingSessionRef.current = null
      setMessages([])
      setAgentTasks([])
    }
  }

  async function selectEditingSession(projectId: string, sessionId: string, knownSessions = editingSessions) {
    activeEditingSessionRef.current = sessionId
    const session = knownSessions.find((candidate) => candidate.id === sessionId)
    if (!session) return null
    const [artifactSnapshot, nextMessages, nextAgentTasks] = await Promise.all([
      artifactWorkspace.loadSession(projectId, sessionId),
      session.conversationId ? listMessages(session.conversationId) : Promise.resolve([]),
      listAgentTasks(projectId, sessionId, session.conversationId ?? undefined),
    ])
    if (activeProjectRef.current !== projectId || activeEditingSessionRef.current !== sessionId) return null
    setEditingSessions(knownSessions)
    setActiveEditingSessionId(sessionId)
    setMessages(nextMessages.map(toMessage))
    setAgentTasks(nextAgentTasks)
    artifactWorkspace.applySessionSnapshot(artifactSnapshot)
    return { session, storyboard: artifactSnapshot.storyboard, timeline: artifactSnapshot.timeline }
  }

  async function createProjectWorkspace(name: string, libraryIds: string[]) {
    if (!desktopRuntime) return
    const project = await createStoredProject(name, libraryIds)
    setProjects((current) => [project, ...current])
    const storedSession = await createStoredEditingSession(project.id, '新的剪辑会话')
    const session = toEditingSession(storedSession)
    setActiveProjectId(project.id)
    activeProjectRef.current = project.id
    await selectEditingSession(project.id, session.id, [session])
  }

  async function refreshEditingSessions(projectId: string) {
    const refreshed = (await listEditingSessions(projectId)).map(toEditingSession)
    if (activeProjectRef.current === projectId) {
      setEditingSessions(refreshed)
    }
    return refreshed
  }

  async function appendStoredMessage(conversationId: string, sessionId: string, role: StoredMessage['role'], content: string, routeReceipt?: string) {
    const storedMessage = await createStoredMessage(conversationId, role, content, routeReceipt)
    if (sessionId === activeEditingSessionRef.current) setMessages((current) => [...current, toMessage(storedMessage)])
    if (role === 'user') setPendingUserMessage(null)
  }

  async function createEditingSessionWorkspace() {
    if (!desktopRuntime) return
    let projectId = activeProjectId
    if (!projectId) {
      const project = await createStoredProject('未命名本地项目')
      setProjects((current) => [project, ...current])
      projectId = project.id
      setActiveProjectId(projectId)
      activeProjectRef.current = projectId
    }
    const storedSession = await createStoredEditingSession(projectId, '新的剪辑会话')
    const session = toEditingSession(storedSession)
    const nextSessions = [session, ...editingSessions]
    setEditingSessions(nextSessions)
    await selectEditingSession(projectId, session.id, nextSessions)
    if (session.conversationId) {
      await appendStoredMessage(session.conversationId, session.id, 'agent', '告诉我你想剪什么。导入素材后，也可以直接说成片目标，我会开始生成第一版。')
      await refreshEditingSessions(projectId)
    }
    setActiveView('chat')
  }

  async function deleteEditingSessionWorkspace(sessionId: string) {
    if (!desktopRuntime || !activeProjectId) return
    const session = editingSessions.find((candidate) => candidate.id === sessionId)
    const title = session?.title ?? '该剪辑会话'
    const confirmed = window.confirm(
      `确定删除「${title}」？\n\n将永久删除该会话的对话、Agent 记录、故事板、时间线和本地预览。项目素材不会删除。`,
    )
    if (!confirmed) return
    const projectId = activeProjectId
    try {
      await deleteStoredEditingSession(projectId, sessionId, true)
    } catch {
      window.alert('删除失败，请稍后重试。')
      return
    }
    const remaining = editingSessions.filter((candidate) => candidate.id !== sessionId)
    setEditingSessions(remaining)
    if (activeEditingSessionRef.current !== sessionId) return
    if (remaining[0]) {
      await selectEditingSession(projectId, remaining[0].id, remaining)
      return
    }
    activeEditingSessionRef.current = null
    setActiveEditingSessionId(null)
    setMessages([])
    setAgentTasks([])
    artifactWorkspace.reset()
  }

  async function ensureEditingSession() {
    let projectId = activeProjectId
    if (!projectId) {
      const project = await createStoredProject('未命名本地项目')
      setProjects((current) => [project, ...current])
      projectId = project.id
      setActiveProjectId(projectId)
      activeProjectRef.current = projectId
    }
    let session = activeEditingSession
    if (!session) {
      const createdSession = toEditingSession(await createStoredEditingSession(projectId, '新的剪辑会话'))
      session = createdSession
      setEditingSessions((current) => [createdSession, ...current])
      setActiveEditingSessionId(session.id)
      activeEditingSessionRef.current = session.id
    }
    let conversationId = session.conversationId
    if (!conversationId) {
      const conversation = await createStoredConversation(projectId, session.id, '新的剪辑会话')
      conversationId = conversation.id
      const updatedSession = { ...session, conversationId }
      session = updatedSession
      setEditingSessions((current) => current.map((candidate) => candidate.id === updatedSession.id ? updatedSession : candidate))
    }
    return { conversationId, projectId, sessionId: session.id }
  }

  async function ensureProject() {
    if (activeProjectId) return activeProjectId
    const project = await createStoredProject('未命名本地项目')
    setProjects((current) => [project, ...current])
    setActiveProjectId(project.id)
    activeProjectRef.current = project.id
    return project.id
  }

  async function resolveMessageContext(request: string) {
    const projectId = await ensureProject()
    const route: TaskRouteResult = await resolveConversationTask(
      projectId,
      activeEditingSessionRef.current,
      request,
    )
    setRouteStatusDetail(route.reasonCode)
    if (route.action === 'clarify') {
      setRouteStatusText('需要任务澄清')
      setRouteStatusTone('warning')
      return { route, context: null }
    }

    let storedSessions = await listEditingSessions(projectId)
    let targetSession: StoredEditingSession | undefined
    if (route.taskId) {
      targetSession = storedSessions.find((candidate) => candidate.id === route.taskId)
    }
    if (!targetSession) throw new Error('Task Resolver did not select an available editing task.')
    const routeReceipt = route.routeReceipt
    if (!routeReceipt) throw new Error('Task Resolver did not authorize the selected editing task.')
    if (!route.conversationId) throw new Error('Task Resolver did not authorize a target conversation.')
    targetSession = { ...targetSession, conversationId: route.conversationId }

    let conversationId = targetSession.conversationId
    if (!conversationId) {
      const conversation = await createStoredConversation(projectId, targetSession.id, targetSession.title)
      conversationId = conversation.id
      targetSession = { ...targetSession, conversationId }
      storedSessions = storedSessions.map((candidate) => candidate.id === targetSession?.id ? targetSession : candidate)
    }
    const nextSessions = storedSessions.map(toEditingSession)
    const selected = await selectEditingSession(projectId, targetSession.id, nextSessions)
    if (!selected) throw new Error('Resolved editing task could not be activated.')
    setRouteStatusText(targetSession.id === activeEditingSessionRef.current ? '已归属到当前任务' : '已切换到匹配任务')
    setRouteStatusTone(targetSession.id === activeEditingSessionRef.current ? 'success' : 'info')
    return {
      route,
      context: {
        conversationId,
        projectId,
        sessionId: targetSession.id,
        storyboardVersionId: selected.storyboard?.id ?? null,
        timelineVersionId: selected.timeline?.id ?? null,
        routeReceipt,
      },
    }
  }

  function showTaskRouteClarification(request: string, question: string) {
    const timestamp = new Date().toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
    const nonce = Date.now()
    setRouteStatusText('需要你确认任务归属')
    setRouteStatusDetail(question)
    setRouteStatusTone('warning')
    setMessages((current) => [
      ...current,
      { id: `task-route-user-${nonce}`, role: 'user', content: request, time: timestamp },
      { id: `task-route-agent-${nonce}`, role: 'agent', content: question, time: timestamp },
    ])
  }

  async function sendMessage() {
    const trimmed = input.trim()
    if (!trimmed || isSending || !desktopRuntime || assetWorkspace.model.importing) return
    const mediaOptions = { ...composerMedia.options }
    cancelRequestedRef.current = false
    setIsSending(true)
    setComposerNotice(null)
    // 任务归属要等一次模型请求；先把用户消息显示出来并清空输入框，落库前失败或取消再放回。
    setPendingUserMessage({ id: `pending-user-${Date.now()}`, role: 'user', content: trimmed, time: '发送中…' })
    setInput('')
    const restoreDraft = () => {
      setPendingUserMessage(null)
      setInput((current) => current.trim() ? current : trimmed)
    }
    let context: { conversationId: string; projectId: string; sessionId: string } | null = null
    let persisted = false
    try {
      if (!await analysisGate.waitForAnalysis() || cancelRequestedRef.current) {
        restoreDraft()
        setIsSending(false)
        setComposerNotice('已取消这次发送，剪辑要求已保留。')
        return
      }
      if (!agentReconciliation.listenerReady && !await agentReconciliation.ensureListener()) {
        restoreDraft()
        setComposerNotice('Agent 事件连接暂时不可用，请再次点击发送重试。')
        setIsSending(false)
        return
      }
      setRouteStatusText('正在确认剪辑任务…')
      setRouteStatusDetail(null)
      setRouteStatusTone('info')
      const resolved = await resolveMessageContext(trimmed)
      if (cancelRequestedRef.current) {
        restoreDraft()
        setIsSending(false)
        setComposerNotice('已停止本轮处理。')
        return
      }
      if (!resolved.context) {
        setPendingUserMessage(null)
        showTaskRouteClarification(trimmed, resolved.route.question || '请确认这条请求属于哪个剪辑任务。')
        setIsSending(false)
        return
      }
      setRouteStatusText(resolved.route.action === 'create_new' ? '将创建新的剪辑任务' : '已完成任务归属')
      setRouteStatusDetail(resolved.route.reasonCode)
      setRouteStatusTone('success')
      context = resolved.context
      const { conversationId, projectId, sessionId } = context
      composerMedia.rememberSent(sessionId, mediaOptions)
      const routedRequest = resolved.route.deferredRequest
        ? `${resolved.route.deferredRequest}\n\n任务归属补充：${trimmed}`
        : trimmed
      await appendStoredMessage(conversationId, sessionId, 'user', routedRequest, resolved.context.routeReceipt)
      persisted = true
      if (cancelRequestedRef.current) {
        setIsSending(false)
        setComposerNotice('已停止本轮处理。')
        return
      }
      await setConversationStatus(conversationId, 'working')
      await refreshEditingSessions(projectId)
      if (cancelRequestedRef.current) {
        await setConversationStatus(conversationId, 'ready')
        await refreshEditingSessions(projectId)
        setIsSending(false)
        setComposerNotice('已停止本轮处理。')
        return
      }
      const turnResult: ConversationTurnResult = await submitConversationTurn(
        projectId,
        sessionId,
        conversationId,
        resolved.context.storyboardVersionId,
        resolved.context.timelineVersionId,
        routedRequest,
        resolved.context.routeReceipt,
        mediaOptions,
      )
      if (turnResult.kind === 'immediate') {
        await appendStoredMessage(conversationId, sessionId, 'agent', turnResult.message)
        await setConversationStatus(conversationId, 'ready')
        await refreshEditingSessions(projectId)
        setIsSending(false)
        return
      }
      const taskId = turnResult.agentTaskId
      if (!taskId) throw new Error('Agent run did not return a task identifier.')
      agentReconciliation.registerPendingEdit({ taskId, projectId, sessionId, conversationId })
      if (cancelRequestedRef.current) {
        await cancelStoredAgentEdit(projectId, sessionId, conversationId, taskId)
      }
      void artifactWorkspace.refreshAudit(
        projectId,
        sessionId,
        conversationId,
      )
    } catch (error) {
      setIsSending(false)
      if (!persisted) restoreDraft()
      setRouteStatusText('这轮请求未完成')
      setRouteStatusDetail(null)
      setRouteStatusTone('warning')
      const errorMessage = error instanceof Error ? error.message : String(error)
      console.error('[App] sendMessage failed:', errorMessage, error)
      let userMessage = context
        ? '这次请求没有完成，请重试；已有结果不会被修改。'
        : '无法准备当前剪辑任务，请重试或重新选择项目。'

      // 提供更具体的错误诊断
      if (errorMessage.includes('Task resolver model is unavailable')) {
        userMessage = 'AI 模型服务暂时不可用。请检查自定义 API 配置或 OAuth 登录状态。'
      } else if (errorMessage.includes('Custom API credential read failed')) {
        userMessage = '自定义 API 凭据读取失败，请检查配置文件。'
      } else if (errorMessage.includes('OAuth not logged in')) {
        userMessage = 'OAuth 未登录或已过期，请重新登录。'
      } else if (errorMessage.includes('Current local project could not be verified')) {
        userMessage = '当前项目不存在或已损坏，请重新选择项目。'
      } else if (errorMessage.includes('Task Resolver did not')) {
        userMessage = `任务归属失败：${errorMessage}`
      }

      setComposerNotice(userMessage)
      if (context) {
        try {
          await appendStoredMessage(context.conversationId, context.sessionId, 'agent', '这次操作没有完成，已有结果没有被修改。请重试，或补充你希望保留的素材和片段。')
          await setConversationStatus(context.conversationId, 'ready')
        } catch {
          // The composer still stays interactive when local persistence is unavailable.
        }
        try {
          await setConversationStatus(context.conversationId, 'ready')
        } catch {
          // A later request or restart can refresh persisted status.
        }
        await refreshEditingSessions(context.projectId)
      }
    }
  }

  function stopAgentRun() {
    if (!isSending) return
    cancelRequestedRef.current = true
    if (analysisGate.waiting) {
      analysisGate.cancel()
      return
    }
    const pending = agentReconciliation.peekPendingEdit()
    if (!pending) {
      setComposerNotice('正在停止…')
      return
    }
    void cancelStoredAgentEdit(
      pending.projectId,
      pending.sessionId,
      pending.conversationId,
      pending.taskId,
    )
      .then(() => setComposerNotice('正在停止本轮处理…'))
      .catch(() => setComposerNotice('停止请求未生效，请稍后再试。'))
  }

  if (!desktopRuntime) {
    return (
      <main className="app-shell browser-notice">
        <section>
          <span className="eyebrow">DESKTOP APP REQUIRED</span>
          <h1>请在 Windows 桌面应用中运行 FellowCut</h1>
          <p>浏览器模式不能访问本地项目、媒体文件、FFmpeg 或 AI 凭据，因此不能用于剪辑测试。</p>
          <code>npm run tauri:dev</code>
        </section>
      </main>
    )
  }

  return (
    <main className="app-shell">
      <AppSidebar
        model={{
          projects,
          activeProjectId,
          sessions: editingSessions,
          activeSessionId: activeEditingSessionId,
          providerLabel: provider.model.providerLabel,
          storeState,
          activeProjectName: activeProject?.name ?? null,
          covers: sessionArtwork?.covers ?? {},
          artworkNotice: sessionArtwork?.notice ?? null,
          view: activeView,
          assetCount: assetWorkspace.page.counts.total,
          analysisStatus: analysisAmbientStatus(assetWorkspace.page.progress),
          analysisHint: analysisSummary(assetWorkspace.page.progress),
        }}
        actions={{
          createSession: () => shotReplacement.actions.requestAction(() => void createEditingSessionWorkspace()),
          createProject: () => shotReplacement.actions.requestAction(() => void projectCreation.actions.open()),
          selectProject: (projectId) => shotReplacement.actions.requestAction(() => void selectProject(projectId)),
          selectSession: (sessionId) => shotReplacement.actions.requestAction(() => { setActiveView('chat'); if (activeProjectId) void selectEditingSession(activeProjectId, sessionId) }),
          deleteSession: (sessionId) => void deleteEditingSessionWorkspace(sessionId),
          renameProject: navigationEditing.actions.renameProject,
          deleteProject: (projectId) => void navigationEditing.actions.deleteProject(projectId),
          renameSession: navigationEditing.actions.renameSession,
          openProvider: provider.actions.open,
          openAssets: () => shotReplacement.actions.requestAction(() => setActiveView('assets')),
        }}
      />

      <section className="workspace">
        <WorkspaceHeader
          model={{
            projectName: activeProject?.name ?? '新项目',
            sessionTitle: activeEditingSession?.title ?? '开始剪辑',
            storeReady: storeState === 'ready',
            view: activeView,
          }}
          actions={{ windowControls, selectView: (view) => shotReplacement.actions.requestAction(() => setActiveView(view)) }}
        />

        <div className="workspace-canvas">
        <ReleaseReadinessBanner enabled={storeState === 'ready'} />
        {activeView !== 'assets' && <header className="cut-heading">
          <div>
            <h1 title={artifactWorkspace.storyboard?.title ?? activeEditingSession?.title}>{artifactWorkspace.storyboard?.title ?? activeEditingSession?.title ?? '从灵感，到画面'}</h1>
            <p>{artifactWorkspace.timeline
              ? `${(artifactWorkspace.timeline.clips.reduce((end, clip) => Math.max(end, clip.timelineEndMs), 0) / 1000).toFixed(1)} 秒 · ${artifactWorkspace.timeline.clips.length} 个镜头 · 第 ${artifactWorkspace.timeline.versionNumber} 版`
              : isSending ? '正在制作你的粗剪…' : '导入素材，开始你的下一段故事'}</p>
            {artifactWorkspace.storyboardVersions.length > 0 && (
              <label className="storyboard-version-picker">
                故事版
                <select
                  value={artifactWorkspace.storyboard?.id ?? ''}
                  onChange={(event) => {
                    const nextId = event.target.value
                    if (nextId) shotReplacement.actions.requestAction(() => void artifactWorkspace.actions.openStoryboard(nextId))
                  }}
                >
                  {artifactWorkspace.storyboardVersions.map((version) => (
                    <option key={version.id} value={version.id}>
                      v{version.versionNumber}
                    </option>
                  ))}
                </select>
              </label>
            )}
          </div>
          <EditorOutputPort
            linkers={artifactWorkspace.model.editorCatalog.linkers}
            selectedId={artifactWorkspace.model.selectedEditorId}
            deliverLabel={artifactWorkspace.model.deliverLabel}
            disabled={!artifactWorkspace.timeline || isSending || artifactWorkspace.model.busy.renderingPreview || shotReplacement.model.phase === 'saving' || shotReplacement.model.phase === 'rendering'}
            busy={artifactWorkspace.model.busy.delivering}
            onSelect={(editorId) => artifactWorkspace.actions.setOutputEditor(editorId)}
            onDeliver={() => shotReplacement.actions.requestAction((timeline) => artifactWorkspace.actions.deliverToEditor(timeline))}
          />
        </header>}
        {activeView === 'assets' && <div className="asset-overlay"><AssetManagementPanel model={assetWorkspace.model} actions={assetWorkspace.actions} /></div>}
        <PairedWorkspace
          hidden={activeView === 'assets'}
          chat={(
            <AgentWorkspace
              model={{
                session: activeEditingSession,
                storyboard: artifactWorkspace.storyboard,
                messages: pendingUserMessage ? [...messages, pendingUserMessage] : messages,
                tasks: agentTasks,
                input,
                isSending,
                editBusy: shotReplacement.model.phase === 'saving',
                listenerReady: agentReconciliation.listenerReady,
                composerNotice,
                mediaOptions: composerMedia.options,
                analysis: {
                  progress: analysisGate.progress ?? assetWorkspace.page.progress,
                  waiting: analysisGate.waiting,
                  importing: assetWorkspace.model.importing,
                },
                routeStatus: { text: routeStatusText, detail: routeStatusDetail, tone: routeStatusTone },
              }}
              actions={{
                setInput: (value) => {
                  setInput(value)
                  if (composerNotice) setComposerNotice(null)
                },
                openArtifacts: () => setActiveView('chat'),
                toggleMedia: composerMedia.toggle,
                sendMessage: (event) => { event.preventDefault(); shotReplacement.actions.requestAction(() => void sendMessage()) },
                stopAgentRun,
              }}
            />
          )}
          preview={(
            <RoughCutPreview
              model={{ artifact: artifactWorkspace.model, replacement: shotReplacement.model, agentBusy: isSending }}
              actions={{ artifact: artifactWorkspace.actions, replacement: shotReplacement.actions }}
            />
          )}
        />
        </div>
      </section>

      <AnalysisIncompleteDialog
        open={analysisGate.prompting}
        progress={analysisGate.progress}
        importing={assetWorkspace.model.importing}
        onUseReady={analysisGate.useReady}
        onCancel={analysisGate.cancel}
        onOpenLibrary={() => {
          analysisGate.cancel()
          shotReplacement.actions.requestAction(() => setActiveView('assets'))
        }}
      />
      <AssetAnalysisModal controller={assetWorkspace.analysis} />
      <ProviderSettingsModal controller={provider} />
      <ProjectCreationModal controller={projectCreation} />
    </main>
  )
}

export default App
