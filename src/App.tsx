// 应用组合根：选择当前项目/任务/会话并装配各领域 controller 与互斥工作区。
import { useEffect, useRef, useState } from 'react'
import type { FormEvent } from 'react'
import './App.css'
import { AgentWorkspace } from './components/AgentWorkspace'
import { AnalysisActivity } from './components/AnalysisActivity'
import { AppSidebar } from './components/AppSidebar'
import { ArtifactsWorkspace } from './components/ArtifactsWorkspace'
import { AssetManagementPanel } from './components/AssetManagementPanel'
import { ProviderSettingsModal } from './components/ProviderSettingsModal'
import { WorkspaceHeader } from './components/WorkspaceHeader'
import type { ConversationMessage, EditingSessionView, WorkspaceView } from './components/workspace-types'
import { useAgentRunReconciliation } from './hooks/useAgentRunReconciliation'
import type { PendingAgentEdit } from './hooks/useAgentRunReconciliation'
import { getTimelineLabel, useArtifactWorkspaceController } from './hooks/useArtifactWorkspaceController'
import { useAssetWorkspaceController } from './hooks/useAssetWorkspaceController'
import { useProviderController } from './hooks/useProviderController'
import {
  createConversation as createStoredConversation,
  createEditingSession as createStoredEditingSession,
  createMessage as createStoredMessage,
  createProject as createStoredProject,
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
  const [projects, setProjects] = useState<StoredProject[]>([])
  const [activeProjectId, setActiveProjectId] = useState<string | null>(null)
  const [editingSessions, setEditingSessions] = useState<EditingSessionView[]>([])
  const [activeEditingSessionId, setActiveEditingSessionId] = useState<string | null>(null)
  const [messages, setMessages] = useState<ConversationMessage[]>([])
  const [activeView, setActiveView] = useState<WorkspaceView>('chat')
  const [input, setInput] = useState('')
  const [isSending, setIsSending] = useState(false)
  const [composerNotice, setComposerNotice] = useState<string | null>(null)
  const [routeStatusText, setRouteStatusText] = useState<string | null>(null)
  const [routeStatusDetail, setRouteStatusDetail] = useState<string | null>(null)
  const [routeStatusTone, setRouteStatusTone] = useState<'neutral' | 'info' | 'success' | 'warning'>('neutral')
  const [agentTasks, setAgentTasks] = useState<StoredAgentTask[]>([])
  const [storeState, setStoreState] = useState<'browser' | 'ready' | 'unavailable'>(desktopRuntime ? 'unavailable' : 'browser')
  const activeProjectRef = useRef<string | null>(null)
  const activeEditingSessionRef = useRef<string | null>(null)
  const activeProject = projects.find((project) => project.id === activeProjectId)
  const activeEditingSession = editingSessions.find((session) => session.id === activeEditingSessionId)
  const provider = useProviderController(desktopRuntime)
  const artifactWorkspace = useArtifactWorkspaceController({
    desktopRuntime,
    projectId: activeProjectId,
    sessionId: activeEditingSessionId,
    session: activeEditingSession,
    activeProjectRef,
    activeSessionRef: activeEditingSessionRef,
    setAgentTasks,
    setMessages,
    appendAgentMessage: (conversationId, sessionId, content) => (
      appendStoredMessage(conversationId, sessionId, 'agent', content)
    ),
    setSessionBrief: (sessionId, brief) => {
      setEditingSessions((current) => current.map((session) => (
        session.id === sessionId ? { ...session, brief } : session
      )))
    },
    selectView: setActiveView,
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

  async function createProjectWorkspace() {
    if (!desktopRuntime) return
    const project = await createStoredProject('未命名本地项目')
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
      await appendStoredMessage(session.conversationId, session.id, 'agent', '这是一个新的剪辑会话。请描述成片目标，或导入素材后直接告诉我生成故事板。')
      await refreshEditingSessions(projectId)
    }
    setActiveView('chat')
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

  async function sendMessage(event: FormEvent) {
    event.preventDefault()
    const trimmed = input.trim()
    if (!trimmed || isSending || !desktopRuntime) return
    setIsSending(true)
    setComposerNotice(null)
    if (!agentReconciliation.listenerReady && !await agentReconciliation.ensureListener()) {
      setComposerNotice('Agent 事件连接暂时不可用，请再次点击发送重试。')
      setIsSending(false)
      return
    }
    let context: { conversationId: string; projectId: string; sessionId: string } | null = null
    try {
      const resolved = await resolveMessageContext(trimmed)
      if (!resolved.context) {
        showTaskRouteClarification(trimmed, resolved.route.question || '请确认这条请求属于哪个剪辑任务。')
        setInput('')
        setIsSending(false)
        return
      }
      setRouteStatusText(resolved.route.action === 'create_new' ? '将创建新的剪辑任务' : '已完成任务归属')
      setRouteStatusDetail(resolved.route.reasonCode)
      setRouteStatusTone('success')
      context = resolved.context
      const { conversationId, projectId, sessionId } = context
      const routedRequest = resolved.route.deferredRequest
        ? `${resolved.route.deferredRequest}\n\n任务归属补充：${trimmed}`
        : trimmed
      await appendStoredMessage(conversationId, sessionId, 'user', routedRequest, resolved.context.routeReceipt)
      await setConversationStatus(conversationId, 'working')
      await refreshEditingSessions(projectId)
      setInput('')
      const turnResult: ConversationTurnResult = await submitConversationTurn(
        projectId,
        sessionId,
        conversationId,
        resolved.context.storyboardVersionId,
        resolved.context.timelineVersionId,
        routedRequest,
        resolved.context.routeReceipt,
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
      void artifactWorkspace.refreshAudit(
        projectId,
        sessionId,
        conversationId,
        resolved.context.storyboardVersionId,
      )
    } catch (error) {
      setIsSending(false)
      setRouteStatusText('这轮请求未完成')
      setRouteStatusDetail(null)
      setRouteStatusTone('warning')
      const errorMessage = error instanceof Error ? error.message : String(error)
      console.error('[App] sendMessage failed:', errorMessage, error)
      let userMessage = context
        ? '这次请求没有完成，请重试；现有 storyboard、时间线和 preview 未被修改。'
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
          await appendStoredMessage(context.conversationId, context.sessionId, 'agent', '这次受限操作没有完成，我没有修改现有 storyboard、时间线或 preview。请重试，或补充你希望保留的素材和片段。')
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

  if (!desktopRuntime) {
    return (
      <main className="app-shell browser-notice">
        <section>
          <span className="eyebrow">DESKTOP APP REQUIRED</span>
          <h1>请在 Windows 桌面应用中运行 Assembly Video Agent</h1>
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
        }}
        actions={{
          createSession: () => void createEditingSessionWorkspace(),
          createProject: () => void createProjectWorkspace(),
          selectProject: (projectId) => void selectProject(projectId),
          selectSession: (sessionId) => { if (activeProjectId) void selectEditingSession(activeProjectId, sessionId) },
          openProvider: provider.actions.open,
        }}
      />

      <section className="workspace">
        <WorkspaceHeader
          model={{
            projectName: activeProject?.name ?? '新 local project',
            sessionTitle: activeEditingSession?.title ?? '开始剪辑会话',
            storeReady: storeState === 'ready',
            view: activeView,
            assetCount: assetWorkspace.page.counts.total,
            shotCount: artifactWorkspace.storyboard?.shots.length ?? 0,
            hasStoryboard: Boolean(artifactWorkspace.storyboard),
            timelineLabel: getTimelineLabel(artifactWorkspace.timelineState, artifactWorkspace.timeline),
          }}
          selectView={setActiveView}
        />

        {activeView === 'assets' && (
          <AssetManagementPanel model={assetWorkspace.model} actions={assetWorkspace.actions} />
        )}
        {activeView === 'chat' && (
          <AgentWorkspace
            model={{
              session: activeEditingSession,
              storyboard: artifactWorkspace.storyboard,
              messages,
              tasks: agentTasks,
              input,
              isSending,
              listenerReady: agentReconciliation.listenerReady,
              composerNotice,
              routeStatus: { text: routeStatusText, detail: routeStatusDetail, tone: routeStatusTone },
            }}
            actions={{
              setInput: (value) => {
                setInput(value)
                if (composerNotice) setComposerNotice(null)
              },
              openArtifacts: () => setActiveView('artifacts'),
              sendMessage,
              confirmStoryboard: async () => {
                if (!activeProjectId || !activeEditingSession || !artifactWorkspace.storyboard) return
                const projectId = activeProjectId
                const sessionId = activeEditingSessionId
                const conversationId = activeEditingSession.conversationId
                if (!sessionId || !conversationId) return
                setIsSending(true)
                try {
                  // confirmStoryboard 返回后台任务 ID，后端线程仍在执行 timeline + preview。
                  // 对齐 sendMessage 异步路径：设为 working 状态 → 注册 pendingEdit → 由
                  // reconciliation 完成时调 setIsSending(false)，而非 finally 立刻重置。
                  const taskId = await artifactWorkspace.actions.confirmStoryboard()
                  await setConversationStatus(conversationId, 'working')
                  await refreshEditingSessions(projectId)
                  agentReconciliation.registerPendingEdit({ taskId, projectId, sessionId, conversationId })
                } catch (error) {
                  console.error('Storyboard confirmation failed:', error)
                  setComposerNotice('确认失败，请重试或在对话中说明')
                  setIsSending(false)
                }
              },
            }}
          />
        )}
        {activeView === 'artifacts' && (
          <ArtifactsWorkspace
            model={{
              ...artifactWorkspace.model,
              assetCounts: assetWorkspace.page.counts,
              assets: assetWorkspace.assets.map((asset) => ({ id: asset.id, name: asset.name })),
              tasks: agentTasks,
            }}
            actions={{
              ...artifactWorkspace.actions,
              adjustShot: (orderIndex) => {
                setActiveView('chat')
                setInput(`调整第 ${orderIndex} 个镜头：`)
              },
            }}
          />
        )}
      </section>

      <AnalysisActivity
        analyzingCount={assetWorkspace.page.counts.analyzing}
        queuedCount={assetWorkspace.page.counts.queued}
        visibleAssets={assetWorkspace.assets}
      />
      <ProviderSettingsModal controller={provider} />
    </main>
  )
}

export default App
