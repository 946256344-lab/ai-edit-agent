import { useEffect, useRef, useState } from 'react'
import type { FormEvent } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { convertFileSrc } from '@tauri-apps/api/core'
import { openUrl } from '@tauri-apps/plugin-opener'
import { listen } from '@tauri-apps/api/event'
import './App.css'
import { ConversationWorkspace } from './components/ConversationWorkspace'
import { AssetManagementPanel } from './components/AssetManagementPanel'
import type { AssetView } from './components/asset-workspace/AssetBrowser'
import {
  clearCustomApi,
  clearExperimentalOpenAIOAuth,
  confirmAssetRelink,
  createConversation as createStoredConversation,
  createEditingSession as createStoredEditingSession,
  createJianyingDraft,
  createMessage as createStoredMessage,
  createProject as createStoredProject,
  createTimelineDraft,
  generateStoryboard as generateStoredStoryboard,
  getAssetEvidence,
  getAssetHealthScanSummary,
  startAssetHealthScan,
  cancelAssetHealthScan,
  getCustomApiStatus,
  getExperimentalOpenAIOAuthStatus,
  getJianyingRegistrationStatus,
  getLatestStoryboard,
  getLatestTimeline,
  importAssetFolder as importStoredAssetFolder,
  importAssets as importStoredAssets,
  initializeLocalStore,
  isDesktopRuntime,
  listAgentTasks,
  listAssetPage,
  listEditingSessions,
  listMessages,
  listOperationLogs,
  listProjects,
  listTimelineVersions,
  previewAssetRelink,
  renderPreview,
  resolveConversationTask,
  saveCustomApi,
  setConversationStatus,
  startExperimentalOpenAIOAuth,
  submitConversationTurn,
} from './lib/local-store'
import type { AgentEditEvent, AssetEvidence, AssetHealthScanSummary, AssetPage, AssetRelinkPreview, ConversationTurnResult, CustomApiStatus, ExperimentalOAuthStatus, JianyingRegistrationStatus, PreviewResult, StoryboardVersion, StoredAgentTask, StoredAsset, StoredEditingSession, StoredMessage, StoredOperationLog, StoredProject, TaskRouteResult, TimelineVersion } from './lib/local-store'

type EditingSession = {
  id: string
  conversationId: string | null
  title: string
  preview: string
  brief: string
  updated: string
  state: 'ready' | 'working' | 'review'
}

type Message = {
  id: string
  role: 'agent' | 'user'
  content: string
  time: string
}

function toEditingSession(session: StoredEditingSession): EditingSession {
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

function toMessage(message: StoredMessage): Message {
  return {
    id: message.id,
    role: message.role === 'user' ? 'user' : 'agent',
    content: message.content,
    time: new Date(message.createdAt).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' }),
  }
}

function toAsset(asset: StoredAsset): AssetView {
  const kind = asset.kind === 'video' || asset.kind === 'image' || asset.kind === 'audio' ? asset.kind : 'other'
  return {
    id: asset.id,
    name: asset.displayName,
    folderName: asset.folderName,
    relativePath: asset.relativePath,
    kind,
    duration: formatDuration(asset.durationMs),
    status: asset.analysisStatus === 'ready' ? 'ready' : asset.analysisStatus === 'failed' ? 'failed' : asset.analysisStatus === 'queued' ? 'queued' : 'analyzing',
    visualStatus: asset.visualAnalysisStatus,
    sourceHealthStatus: asset.sourceHealthStatus,
    thumbnailUrl: asset.thumbnailPath && isDesktopRuntime() ? convertFileSrc(asset.thumbnailPath) : null,
  }
}

function formatDuration(durationMs: number | null) {
  if (durationMs === null) return ''
  const seconds = Math.floor(durationMs / 1000)
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
}

type PendingAgentEdit = {
  taskId: string
  projectId: string
  sessionId: string
  conversationId: string
}


const ACTIVE_AGENT_TASK_STATUSES = new Set<StoredAgentTask['status']>(['queued', 'running'])

function isActiveAgentTask(task: StoredAgentTask) {
  return ACTIVE_AGENT_TASK_STATUSES.has(task.status)
}

function isTerminalAgentTask(task: StoredAgentTask) {
  return !isActiveAgentTask(task)
}

function App() {
  const desktopRuntime = isDesktopRuntime()
  const [projects, setProjects] = useState<StoredProject[]>([])
  const [activeProjectId, setActiveProjectId] = useState<string | null>(null)
  const [editingSessions, setEditingSessions] = useState<EditingSession[]>([])
  const [activeEditingSessionId, setActiveEditingSessionId] = useState<string | null>(null)
  const [assets, setAssets] = useState<AssetView[]>([])
  const [assetPage, setAssetPage] = useState<Pick<AssetPage, 'total' | 'directories' | 'unfiledCount' | 'counts'>>({ total: 0, directories: [], unfiledCount: 0, counts: { total: 0, ready: 0, analyzing: 0, queued: 0, failed: 0 } })
  const [assetPageRevision, setAssetPageRevision] = useState(0)
  const [assetHealth, setAssetHealth] = useState<AssetHealthScanSummary | null>(null)
  const [assetRelinkPreview, setAssetRelinkPreview] = useState<AssetRelinkPreview | null>(null)
  const [assetRelinkSourceDirectory, setAssetRelinkSourceDirectory] = useState<string | null>(null)
  const [assetFolderFilter, setAssetFolderFilter] = useState('all')
  const [assetEvidence, setAssetEvidence] = useState<AssetEvidence | null>(null)
  const [storyboard, setStoryboard] = useState<StoryboardVersion | null>(null)
  const [storyboardBrief, setStoryboardBrief] = useState('')
  const [storyboardError, setStoryboardError] = useState<string | null>(null)
  const [isGeneratingStoryboard, setIsGeneratingStoryboard] = useState(false)
  const [messages, setMessages] = useState<Message[]>([])
  const [activeView, setActiveView] = useState<'chat' | 'assets' | 'artifacts'>('chat')
  const [input, setInput] = useState('')
  const [isSending, setIsSending] = useState(false)
  const [composerNotice, setComposerNotice] = useState<string | null>(null)
  const [routeStatusText, setRouteStatusText] = useState<string | null>(null)
  const [routeStatusDetail, setRouteStatusDetail] = useState<string | null>(null)
  const [routeStatusTone, setRouteStatusTone] = useState<'neutral' | 'info' | 'success' | 'warning'>('neutral')
  const [agentEditListenerReady, setAgentEditListenerReady] = useState(!desktopRuntime)
  const [providerOpen, setProviderOpen] = useState(false)
  const [timelineState, setTimelineState] = useState<'not-created' | 'draft' | 'preview-generating' | 'preview-ready' | 'jianying-pending' | 'jianying'>('not-created')
  const [timeline, setTimeline] = useState<TimelineVersion | null>(null)
  const [preview, setPreview] = useState<PreviewResult | null>(null)
  const [previewNonce, setPreviewNonce] = useState(0)
  const [isCreatingTimeline, setIsCreatingTimeline] = useState(false)
  const [isRenderingPreview, setIsRenderingPreview] = useState(false)
  const [isCreatingJianyingDraft, setIsCreatingJianyingDraft] = useState(false)
  const [deliveryStatus, setDeliveryStatus] = useState<string>('等待生成故事板')
  const [agentTasks, setAgentTasks] = useState<StoredAgentTask[]>([])
  const [operationLogs, setOperationLogs] = useState<StoredOperationLog[]>([])
  const [timelineVersions, setTimelineVersions] = useState<TimelineVersion[]>([])
  const [storeState, setStoreState] = useState<'browser' | 'ready' | 'unavailable'>(desktopRuntime ? 'unavailable' : 'browser')
  const [oauthStatus, setOAuthStatus] = useState<ExperimentalOAuthStatus>({ state: 'disconnected', message: null, experimental: true })
  const [customApiStatus, setCustomApiStatus] = useState<CustomApiStatus>({ state: 'disconnected', message: null, baseUrl: null, model: null, coarseVisualModel: null })
  const [customBaseUrl, setCustomBaseUrl] = useState('')
  const [customModel, setCustomModel] = useState('')
  const [customCoarseVisualModel, setCustomCoarseVisualModel] = useState('')
  const [customApiKey, setCustomApiKey] = useState('')
  const [isSavingCustomApi, setIsSavingCustomApi] = useState(false)
  const activeProjectRef = useRef<string | null>(null)
  const activeEditingSessionRef = useRef<string | null>(null)
  const activeTimelineRef = useRef<string | null>(null)
  const pendingEditRef = useRef<PendingAgentEdit | null>(null)
  const earlyAgentEditEventsRef = useRef<Map<string, AgentEditEvent>>(new Map())
  const observedActiveAgentTaskIdsRef = useRef<Set<string>>(new Set())
  const reconcilingAgentTaskIdsRef = useRef<Set<string>>(new Set())
  const reconciledAgentTaskIdsRef = useRef<Set<string>>(new Set())
  const agentEditUnlistenRef = useRef<(() => void) | null>(null)
  const agentEditListenerPromiseRef = useRef<Promise<boolean> | null>(null)
  const activeProject = projects.find((project) => project.id === activeProjectId)
  const activeEditingSession = editingSessions.find((session) => session.id === activeEditingSessionId)
  const analyzingAssets = assets.filter((asset) => asset.status === 'analyzing')
  function applyPreview(nextPreview: PreviewResult | null) {
    if (nextPreview) setPreviewNonce((nonce) => nonce + 1)
    setPreview(nextPreview)
    setIsRenderingPreview(false)
  }

  async function applyAgentEditCompletion(pending: PendingAgentEdit, event?: AgentEditEvent) {
    const { projectId, sessionId } = pending
    const result = event?.result
    const isActiveScope = activeProjectRef.current === projectId
      && activeEditingSessionRef.current === sessionId
    if (isActiveScope && result) {
      if (result.storyboard) {
        setStoryboard(result.storyboard)
        setStoryboardBrief(result.storyboard.brief)
        setTimeline(null)
        applyPreview(null)
        setTimelineState('not-created')
        setDeliveryStatus('故事板已生成 · 可以创建时间线')
      }
      if (result.timeline) {
        setTimeline(result.timeline)
        setTimelineState(
          result.jianyingDraft?.registrationStatus === 'pending'
            ? 'jianying-pending'
            : result.jianyingDraft
              ? 'jianying'
              : result.preview
                ? 'preview-ready'
                : 'draft',
        )
        setDeliveryStatus(result.preview ? '预览已生成 · 可交付草稿' : '内部时间线已生成')
      }
      if (result.preview) applyPreview(result.preview)
    }
    const refreshedSessions = await refreshEditingSessions(projectId)
    if (activeProjectRef.current !== projectId || activeEditingSessionRef.current !== sessionId) return
    await selectEditingSession(projectId, sessionId, refreshedSessions)
  }

  function reconcileAgentCompletion(pending: PendingAgentEdit, event?: AgentEditEvent) {
    const taskId = pending.taskId
    if (reconciledAgentTaskIdsRef.current.has(taskId) || reconcilingAgentTaskIdsRef.current.has(taskId)) return
    const controlsComposer = pendingEditRef.current?.taskId === taskId
    reconcilingAgentTaskIdsRef.current.add(taskId)
    if (controlsComposer) pendingEditRef.current = null
    earlyAgentEditEventsRef.current.delete(taskId)
    void applyAgentEditCompletion(pending, event)
      .then(() => {
        const reconciled = reconciledAgentTaskIdsRef.current
        reconciled.add(taskId)
        if (reconciled.size > 100) {
          const oldestTaskId = reconciled.values().next().value
          if (oldestTaskId) reconciled.delete(oldestTaskId)
        }
        observedActiveAgentTaskIdsRef.current.delete(taskId)
      })
      .catch(() => {
        setComposerNotice('Agent 已完成，但界面状态同步失败；请切换任务或重启应用后查看持久化结果。')
      })
      .finally(() => {
        reconcilingAgentTaskIdsRef.current.delete(taskId)
        if (controlsComposer) setIsSending(false)
      })
  }

  function receiveAgentEditCompletion(event: AgentEditEvent) {
    if (reconciledAgentTaskIdsRef.current.has(event.agentTaskId)) return
    const pending = pendingEditRef.current
    if (!pending || event.agentTaskId !== pending.taskId) {
      const earlyEvents = earlyAgentEditEventsRef.current
      earlyEvents.set(event.agentTaskId, event)
      if (earlyEvents.size > 20) {
        const oldestTaskId = earlyEvents.keys().next().value
        if (oldestTaskId) earlyEvents.delete(oldestTaskId)
      }
      return
    }
    reconcileAgentCompletion(pending, event)
  }

  function ensureAgentEditListener() {
    if (!desktopRuntime || agentEditUnlistenRef.current) return Promise.resolve(true)
    if (agentEditListenerPromiseRef.current) return agentEditListenerPromiseRef.current
    const listenerPromise = listen<AgentEditEvent>('agent-edit-completed', (event) => receiveAgentEditCompletion(event.payload))
      .then((unlisten) => {
        agentEditUnlistenRef.current = unlisten
        setAgentEditListenerReady(true)
        return true
      })
      .catch(() => {
        setAgentEditListenerReady(false)
        return false
      })
      .finally(() => {
        agentEditListenerPromiseRef.current = null
      })
    agentEditListenerPromiseRef.current = listenerPromise
    return listenerPromise
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
  }, [desktopRuntime])

  useEffect(() => {
    if (!desktopRuntime || !activeProjectId || !activeEditingSessionId || !activeEditingSession?.conversationId) return
    if (!isSending && activeEditingSession.state !== 'working') return
    const projectId = activeProjectId
    const sessionId = activeEditingSessionId
    const conversationId = activeEditingSession.conversationId
    let cancelled = false
    for (const task of agentTasks) {
      if (isActiveAgentTask(task)) observedActiveAgentTaskIdsRef.current.add(task.id)
    }
    const refresh = () => void listAgentTasks(projectId, sessionId, conversationId)
      .then((nextTasks) => {
        if (cancelled || activeProjectRef.current !== projectId || activeEditingSessionRef.current !== sessionId) return
        setAgentTasks(nextTasks)
        for (const task of nextTasks) {
          if (isActiveAgentTask(task)) observedActiveAgentTaskIdsRef.current.add(task.id)
        }
        const pending = pendingEditRef.current
        if (pending && pending.projectId === projectId && pending.sessionId === sessionId) {
          const terminalPending = nextTasks.find((task) => task.id === pending.taskId && isTerminalAgentTask(task))
          if (terminalPending) {
            reconcileAgentCompletion(pending, earlyAgentEditEventsRef.current.get(pending.taskId))
            return
          }
          return
        }
        const observedTerminal = nextTasks.find((task) => (
          isTerminalAgentTask(task)
          && observedActiveAgentTaskIdsRef.current.has(task.id)
          && !reconciledAgentTaskIdsRef.current.has(task.id)
        ))
        const snapshotHasActiveTask = nextTasks.some(isActiveAgentTask)
        const persistedWorkingTerminal = !isSending && activeEditingSession.state === 'working' && !snapshotHasActiveTask
          ? nextTasks.find((task) => (
              isTerminalAgentTask(task)
              && !reconciledAgentTaskIdsRef.current.has(task.id)
            ))
          : undefined
        const terminalToReconcile = observedTerminal ?? persistedWorkingTerminal
        if (terminalToReconcile) {
          reconcileAgentCompletion(
            { taskId: terminalToReconcile.id, projectId, sessionId, conversationId },
            earlyAgentEditEventsRef.current.get(terminalToReconcile.id),
          )
        }
      })
      .catch(() => undefined)
    refresh()
    const intervalId = window.setInterval(refresh, 1200)
    return () => {
      cancelled = true
      window.clearInterval(intervalId)
    }
    // Composer ownership or the persisted working state keeps this poll alive until
    // reconciliation finishes; task status changes never control the interval lifetime.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [activeEditingSession?.conversationId, activeEditingSession?.state, activeEditingSessionId, activeProjectId, desktopRuntime, isSending])

  useEffect(() => {
    if (!desktopRuntime || !isSending) return
    const pending = pendingEditRef.current
    if (!pending) return
    const pendingTask = agentTasks.find((task) => task.id === pending.taskId)
    if (pendingTask && isTerminalAgentTask(pendingTask)) {
      reconcileAgentCompletion(pending, earlyAgentEditEventsRef.current.get(pending.taskId))
    }
  }, [agentTasks, desktopRuntime, isSending])

  useEffect(() => {
    activeTimelineRef.current = timeline?.id ?? null
  }, [timeline])

  useEffect(() => {
    if (!storyboard) {
      setDeliveryStatus('等待生成故事板')
    } else if (!timeline) {
      setDeliveryStatus('故事板已就绪 · 可创建内部时间线')
    } else if (timelineState === 'preview-generating') {
      setDeliveryStatus('预览生成中')
    } else if (timelineState === 'jianying-pending') {
      setDeliveryStatus('预览已完成 · 等待剪映注册')
    } else if (timelineState === 'jianying') {
      setDeliveryStatus('草稿已交付到剪映')
    } else if (!preview) {
      setDeliveryStatus('内部时间线已就绪 · 可生成预览')
    } else {
      setDeliveryStatus('预览已就绪 · 可交付草稿')
    }
  }, [storyboard, timeline, preview, timelineState])

  useEffect(() => {
    if (!desktopRuntime) return
    let stopListening: (() => void) | undefined
    void listen<JianyingRegistrationStatus>('jianying-draft-registration-status', (event) => {
      if (event.payload.timelineVersionId !== activeTimelineRef.current) return
      setTimelineState(event.payload.status === 'registered' ? 'jianying' : event.payload.status === 'pending' ? 'jianying-pending' : 'draft')
    }).then((unlisten) => { stopListening = unlisten })
    return () => stopListening?.()
  }, [desktopRuntime])

  useEffect(() => {
    if (!desktopRuntime) return
    let stopListening: (() => void) | undefined
    const refreshStatus = () => void getExperimentalOpenAIOAuthStatus()
      .then(setOAuthStatus)
      .catch(() => setOAuthStatus({ state: 'failed', message: '无法读取 OAuth 状态。', experimental: true }))
    void listen<ExperimentalOAuthStatus>('experimental-openai-oauth-status', (event) => setOAuthStatus(event.payload))
      .then((unlisten) => { stopListening = unlisten })
    refreshStatus()
    const intervalId = window.setInterval(refreshStatus, 2000)
    return () => {
      window.clearInterval(intervalId)
      stopListening?.()
    }
  }, [desktopRuntime])

  useEffect(() => {
    if (!desktopRuntime) return
    void refreshCustomApiStatus()
  }, [desktopRuntime])

  useEffect(() => {
    if (!desktopRuntime || !activeProjectId) return
    const projectId = activeProjectId
    let cancelled = false
    const refreshAssets = () => {
      void listAssetPage(projectId, {
        directoryKey: assetFolderFilter === 'all' ? undefined : assetFolderFilter,
        offset: 0,
        limit: 100,
      }).then((page) => {
        if (!cancelled && activeProjectRef.current === projectId) {
          const nextItems = page.items.map(toAsset)
          setAssets(nextItems)
          setAssetPage({ total: page.total, directories: page.directories, unfiledCount: page.unfiledCount, counts: page.counts })
        }
      }).catch(() => undefined)
    }
    refreshAssets()
    const intervalId = window.setInterval(refreshAssets, 1500)
    return () => { cancelled = true; window.clearInterval(intervalId) }
  }, [activeProjectId, assetFolderFilter, assetPageRevision, desktopRuntime])

  useEffect(() => {
    if (!desktopRuntime || !activeProjectId) return
    const projectId = activeProjectId
    const refresh = () => void getAssetHealthScanSummary(projectId).then((summary) => {
      if (activeProjectRef.current === projectId) setAssetHealth(summary)
    }).catch(() => undefined)
    refresh(); const intervalId = window.setInterval(refresh, 2000)
    return () => window.clearInterval(intervalId)
  }, [activeProjectId, desktopRuntime])

  useEffect(() => {
    if (!desktopRuntime) return
    const restoreListener = () => void ensureAgentEditListener()
    restoreListener()
    window.addEventListener('focus', restoreListener)
    const retryInterval = window.setInterval(restoreListener, 3000)
    return () => {
      window.removeEventListener('focus', restoreListener)
      window.clearInterval(retryInterval)
      agentEditUnlistenRef.current?.()
      agentEditUnlistenRef.current = null
      setAgentEditListenerReady(false)
    }
    // The listener is intentionally process-scoped; message handlers read current scope from refs.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [desktopRuntime])

  async function selectProject(projectId: string) {    activeProjectRef.current = projectId
    setActiveProjectId(projectId)
    const storedSessions = await listEditingSessions(projectId)
    if (activeProjectRef.current !== projectId) return
    const nextSessions = storedSessions.map(toEditingSession)
    setEditingSessions(nextSessions)
    setAssets([])
    setAssetPage({ total: 0, directories: [], unfiledCount: 0, counts: { total: 0, ready: 0, analyzing: 0, queued: 0, failed: 0 } })
    setAssetFolderFilter('all')
    setAssetHealth(null)
    setAssetRelinkPreview(null)
    setAssetRelinkSourceDirectory(null)
    setAssetEvidence(null)
    if (nextSessions[0]) await selectEditingSession(projectId, nextSessions[0].id, nextSessions)
    else {
      setActiveEditingSessionId(null)
      activeEditingSessionRef.current = null
      setMessages([])
      setStoryboard(null)
      setTimeline(null)
      setPreview(null)
      setAgentTasks([])
      setOperationLogs([])
      setTimelineVersions([])
      setTimelineState('not-created')
    }
  }

  async function selectEditingSession(projectId: string, sessionId: string, knownSessions = editingSessions) {
    activeEditingSessionRef.current = sessionId
    const session = knownSessions.find((candidate) => candidate.id === sessionId)
    if (!session) return null
    const latestStoryboard = await getLatestStoryboard(projectId, sessionId)
    const [latestTimeline, nextMessages, nextAgentTasks, nextOperationLogs, nextTimelineVersions] = await Promise.all([
      latestStoryboard ? getLatestTimeline(projectId, latestStoryboard.id) : Promise.resolve(null),
      session.conversationId ? listMessages(session.conversationId) : Promise.resolve([]),
      listAgentTasks(projectId, sessionId, session.conversationId ?? undefined),
      listOperationLogs(projectId, sessionId),
      latestStoryboard ? listTimelineVersions(projectId, sessionId, latestStoryboard.id) : Promise.resolve([]),
    ])
    const registration = latestTimeline
      ? await getJianyingRegistrationStatus(latestTimeline.timeline.id)
      : null
    if (activeProjectRef.current !== projectId || activeEditingSessionRef.current !== sessionId) return null
    setEditingSessions(knownSessions)
    setActiveEditingSessionId(sessionId)
    setMessages(nextMessages.map(toMessage))
    setStoryboard(latestStoryboard)
    setStoryboardBrief(latestStoryboard?.brief ?? '')
    setTimeline(latestTimeline?.timeline ?? null)
    applyPreview(latestTimeline?.preview ?? null)
    setAgentTasks(nextAgentTasks)
    setOperationLogs(nextOperationLogs)
    setTimelineVersions(nextTimelineVersions)
    const nextTimelineState = registration?.status === 'pending'
      ? 'jianying-pending'
      : registration?.status === 'registered'
        ? 'jianying'
        : latestTimeline?.preview
          ? 'preview-ready'
          : latestTimeline
            ? 'draft'
            : 'not-created'
    setTimelineState(nextTimelineState)
    setDeliveryStatus(
      latestTimeline?.preview
        ? nextTimelineState === 'jianying'
          ? '草稿已交付到剪映'
          : nextTimelineState === 'jianying-pending'
            ? '预览已完成 · 等待剪映注册'
            : '预览已就绪 · 可交付草稿'
        : latestTimeline
          ? '内部时间线已生成'
          : latestStoryboard
            ? '故事板已就绪 · 可创建内部时间线'
            : '等待生成故事板',
    )
    return { session, storyboard: latestStoryboard, timeline: latestTimeline?.timeline ?? null }
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

  async function connectExperimentalOpenAI() {
    try {
      const start = await startExperimentalOpenAIOAuth()
      setOAuthStatus({ state: 'pending', message: '请在浏览器中完成登录。', experimental: start.experimental })
      await openUrl(start.authorizationUrl)
    } catch {
      setOAuthStatus({ state: 'failed', message: '无法启动实验性 OAuth 登录。', experimental: true })
    }
  }

  async function disconnectExperimentalOpenAI() {
    try {
      setOAuthStatus(await clearExperimentalOpenAIOAuth())
    } catch {
      setOAuthStatus({ state: 'failed', message: '退出登录失败。', experimental: true })
    }
  }

  async function refreshCustomApiStatus() {
    const status = await getCustomApiStatus().catch(() => ({ state: 'failed' as const, message: '无法读取自定义 API 状态。', baseUrl: null, model: null, coarseVisualModel: null }))
    setCustomApiStatus(status)
  }

  async function saveCustomConnection(baseUrl: string, model: string, coarseVisualModel: string, apiKey: string) {
    const status = await saveCustomApi(baseUrl.trim(), model.trim(), coarseVisualModel.trim(), apiKey.trim()).catch(() => ({ state: 'failed' as const, message: '保存自定义 API 凭据失败。', baseUrl: null, model: null, coarseVisualModel: null }))
    setCustomApiStatus(status)
    return status.state === 'connected'
  }

  async function disconnectCustomApi() {
    try {
      setCustomApiStatus(await clearCustomApi())
    } catch {
      setCustomApiStatus({ state: 'failed', message: '清除自定义 API 失败。', baseUrl: null, model: null, coarseVisualModel: null })
    }
  }

  async function refreshEditingSessions(projectId: string) {
    const refreshed = (await listEditingSessions(projectId)).map(toEditingSession)
    if (activeProjectRef.current === projectId) {
      setEditingSessions(refreshed)
    }
    return refreshed
  }

  async function refreshAgentAudit(projectId: string, sessionId: string, conversationId: string, storyboardVersionId: string | null) {
    const [nextTasks, nextLogs, nextTimelineVersions] = await Promise.all([
      listAgentTasks(projectId, sessionId, conversationId),
      listOperationLogs(projectId, sessionId),
      storyboardVersionId ? listTimelineVersions(projectId, sessionId, storyboardVersionId) : Promise.resolve([]),
    ])
    if (activeProjectRef.current === projectId && activeEditingSessionRef.current === sessionId) {
      setAgentTasks(nextTasks)
      setOperationLogs(nextLogs)
      setTimelineVersions(nextTimelineVersions)
    }
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

  async function importAssets() {
    if (!desktopRuntime) return
    const context = activeProjectId && activeEditingSession?.conversationId
      ? { projectId: activeProjectId, conversationId: activeEditingSession.conversationId, sessionId: activeEditingSession.id }
      : await ensureEditingSession()
    const projectId = context.projectId
    const selected = await open({
      multiple: true,
      filters: [{ name: 'Media', extensions: ['mp4', 'mov', 'mkv', 'avi', 'webm', 'm4v', 'jpg', 'jpeg', 'png', 'webp', 'bmp', 'gif', 'mp3', 'wav', 'aac', 'm4a', 'flac', 'ogg'] }],
    })
    if (!selected) return
    const sources = Array.isArray(selected) ? selected : [selected]
    const imported = await importStoredAssets(projectId, sources)
    if (activeProjectRef.current === projectId) {
      setAssetPageRevision((value) => value + 1)
    }
    await appendStoredMessage(context.conversationId, context.sessionId, 'agent', `已将 ${imported.length} 个素材加入本地分析队列。分析完成前不会影响当前故事板。`)
    await refreshEditingSessions(projectId)
  }

  async function importAssetFolder() {
    if (!desktopRuntime) return
    const context = activeProjectId && activeEditingSession?.conversationId
      ? { projectId: activeProjectId, conversationId: activeEditingSession.conversationId, sessionId: activeEditingSession.id }
      : await ensureEditingSession()
    const projectId = context.projectId
    const selected = await open({ directory: true, multiple: false })
    if (!selected || Array.isArray(selected)) return
    const imported = await importStoredAssetFolder(projectId, selected)
    if (activeProjectRef.current === projectId) {
      setAssetPageRevision((value) => value + 1)
    }
    await appendStoredMessage(context.conversationId, context.sessionId, 'agent', `已从文件夹导入 ${imported.length} 个素材。仅支持的媒体文件会加入本地分析队列。`)
    await refreshEditingSessions(projectId)
  }

  async function relinkAssetFolder() {
    if (!desktopRuntime || !activeProjectId) return
    const selected = await open({ directory: true, multiple: false, title: '选择新的素材根目录' })
    if (!selected || Array.isArray(selected)) return
    const preview = await previewAssetRelink(activeProjectId, selected)
    setAssetRelinkPreview(preview)
    setAssetRelinkSourceDirectory(selected)
    if (!preview.matches.length) {
      window.alert('没有找到可安全重链路的素材。请选择保留原有文件夹结构的素材根目录。')
      return
    }
  }

  async function confirmRelinkAssetFolder() {
    if (!desktopRuntime || !activeProjectId || !assetRelinkSourceDirectory || !assetRelinkPreview) return
    const result = await confirmAssetRelink(activeProjectId, assetRelinkSourceDirectory, assetRelinkPreview.matches.map((match) => match.assetId), true)
    if (activeProjectRef.current === activeProjectId) setAssetPageRevision((value) => value + 1)
    window.alert(`已重新链路 ${result.relinkedCount} 个素材并保留分析信息。`)
    setAssetRelinkPreview(null)
    setAssetRelinkSourceDirectory(null)
  }

  function cancelRelinkPreview() {
    setAssetRelinkPreview(null)
    setAssetRelinkSourceDirectory(null)
  }

  async function selectAssetEvidence(assetId: string) {
    if (!desktopRuntime) return
    const projectId = activeProjectRef.current
    try {
      const evidence = await getAssetEvidence(assetId)
      if (activeProjectRef.current === projectId) setAssetEvidence(evidence)
    } catch {
      if (activeProjectRef.current === projectId) setAssetEvidence(null)
    }
  }

  function selectFolder(path: string) {
    setAssetEvidence(null)
    if (path === assetFolderFilter) return
    setAssets([])
    setAssetFolderFilter(path)
  }


  async function generateStoryboard() {
    const brief = storyboardBrief.trim()
    if (!activeProjectId || !activeEditingSessionId || isGeneratingStoryboard || !brief) {
      if (!brief) setStoryboardError('请先描述要制作的视频目标、时长、语言和重点。')
      return
    }
    const projectId = activeProjectId
    const sessionId = activeEditingSessionId
    setIsGeneratingStoryboard(true)
    setStoryboardError(null)
    try {
      const generated = await generateStoredStoryboard(projectId, sessionId, brief)
      if (activeProjectRef.current !== projectId || activeEditingSessionRef.current !== sessionId) return
      setStoryboard(generated)
      setTimeline(null)
      applyPreview(null)
      setTimelineState('not-created')
      setDeliveryStatus('故事板已生成 · 可以创建内部时间线')
      if (activeEditingSession?.conversationId) {
        await appendStoredMessage(activeEditingSession.conversationId, sessionId, 'agent', `已根据当前剪辑会话创建故事板 v${generated.versionNumber}。你可以检查镜头，或继续要求创建草稿和预览。`)
      }
      setEditingSessions((current) => current.map((session) => session.id === sessionId ? { ...session, brief } : session))
      setActiveView('artifacts')
    } catch {
      if (activeProjectRef.current === projectId && activeEditingSessionRef.current === sessionId) {
        setStoryboardError('Agent 未能生成可用 storyboard；没有修改现有版本。请确认素材分析已完成后重试。')
      }
    } finally {
      setIsGeneratingStoryboard(false)
    }
  }

  async function createTimelineFromStoryboard() {
    if (!activeProjectId || !storyboard || isCreatingTimeline) return
    const projectId = activeProjectId
    setIsCreatingTimeline(true)
    try {
      const generatedTimeline = await createTimelineDraft(projectId, storyboard.id)
      if (activeProjectRef.current !== projectId) return
      setTimeline(generatedTimeline)
      activeTimelineRef.current = generatedTimeline.id
      applyPreview(null)
      setTimelineState('draft')
      setDeliveryStatus('内部时间线已生成 · 可预览')
      if (activeEditingSession?.conversationId) {
        await appendStoredMessage(activeEditingSession.conversationId, activeEditingSession.id, 'agent', '内部时间线已生成，你现在可以直接生成预览，或继续要求我调整镜头顺序。')
      }
      await refreshAgentAudit(projectId, activeEditingSessionId ?? '', activeEditingSession?.conversationId ?? '', storyboard.id)
    } finally {
      setIsCreatingTimeline(false)
    }
  }

  async function renderLocalPreview() {
    if (!activeProjectId || !timeline || isRenderingPreview) return
    const projectId = activeProjectId
    setIsRenderingPreview(true)
    setTimelineState('preview-generating')
    try {
      const generatedPreview = await renderPreview(timeline.id)
      if (activeProjectRef.current !== projectId) return
      applyPreview(generatedPreview)
      setTimelineState('preview-ready')
      setDeliveryStatus('预览已生成 · 可交付草稿')
      if (activeEditingSession?.conversationId) {
        await appendStoredMessage(activeEditingSession.conversationId, activeEditingSession.id, 'agent', '本地预览已经生成，你可以先检查节奏、镜头和字幕，再决定是否交付到剪映。')
      }
    } finally {
      setIsRenderingPreview(false)
    }
  }

  async function createJianyingDelivery() {
    if (!activeProjectId || !timeline || isCreatingJianyingDraft) return
    const projectId = activeProjectId
    setIsCreatingJianyingDraft(true)
    try {
      const draft = await createJianyingDraft(timeline.id)
      if (activeProjectRef.current !== projectId) return
      activeTimelineRef.current = timeline.id
      setTimelineState(draft.registrationStatus === 'pending' ? 'jianying-pending' : 'jianying')
      setDeliveryStatus(draft.registrationStatus === 'pending' ? '草稿已生成 · 等待剪映注册' : '草稿已交付到剪映')
      if (activeEditingSession?.conversationId) {
        await appendStoredMessage(activeEditingSession.conversationId, activeEditingSession.id, 'agent', draft.registrationStatus === 'pending' ? '剪映草稿已生成，等待退出剪映后自动注册。' : '剪映草稿已交付，你可以打开剪映继续查看。')
      }
    } finally {
      setIsCreatingJianyingDraft(false)
    }
  }

  async function sendMessage(event: FormEvent) {
    event.preventDefault()
    const trimmed = input.trim()
    if (!trimmed || isSending || !desktopRuntime) return
    setIsSending(true)
    setComposerNotice(null)
    if (!agentEditListenerReady && !await ensureAgentEditListener()) {
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
      pendingEditRef.current = { taskId, projectId, sessionId, conversationId }
      observedActiveAgentTaskIdsRef.current.add(taskId)
      const earlyCompletion = earlyAgentEditEventsRef.current.get(taskId)
      if (earlyCompletion) receiveAgentEditCompletion(earlyCompletion)
      void refreshAgentAudit(projectId, sessionId, conversationId, resolved.context.storyboardVersionId)
    } catch {
      setIsSending(false)
      setRouteStatusText('这轮请求未完成')
      setRouteStatusDetail(null)
      setRouteStatusTone('warning')
      setComposerNotice(context
        ? '这次请求没有完成，请重试；现有 storyboard、时间线和 preview 未被修改。'
        : '无法准备当前剪辑任务，请重试或重新选择项目。')
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
    return <main className="app-shell browser-notice"><section><span className="eyebrow">DESKTOP APP REQUIRED</span><h1>请在 Windows 桌面应用中运行 Assembly Video Agent</h1><p>浏览器模式不能访问本地项目、媒体文件、FFmpeg 或 AI 凭据，因此不能用于剪辑测试。</p><code>npm run tauri:dev</code></section></main>
  }

  const assetWorkspace = (
    <AssetManagementPanel
      model={{
        projectId: activeProjectId,
        storeReady: storeState === 'ready',
        page: { total: assetPage.total, counts: assetPage.counts },
        directories: assetPage.directories,
        unfiledAssetCount: assetPage.unfiledCount,
        assets,
        selectedDirectoryKey: assetFolderFilter,
        health: assetHealth,
        relinkPreview: assetRelinkPreview,
        hasRelinkSource: Boolean(assetRelinkSourceDirectory),
        evidence: assetEvidence,
      }}
      actions={{
        selectDirectory: selectFolder,
        inspectAsset: (assetId) => void selectAssetEvidence(assetId),
        closeEvidence: () => setAssetEvidence(null),
        importFiles: () => void importAssets(),
        importFolder: () => void importAssetFolder(),
        startHealthScan: () => { if (activeProjectId) void startAssetHealthScan(activeProjectId) },
        cancelHealthScan: (taskId) => { if (activeProjectId) void cancelAssetHealthScan(activeProjectId, taskId) },
        openRelink: () => void relinkAssetFolder(),
        confirmRelink: () => void confirmRelinkAssetFolder(),
        cancelRelink: cancelRelinkPreview,
      }}
    />
  )

  return (
    <main className="app-shell">
      <aside className="sidebar">
        <div className="brand"><span className="brand-mark">A</span><span>ASSEMBLY</span><small>VIDEO AGENT</small></div>
        <button className="new-chat" onClick={() => void createEditingSessionWorkspace()}>+ 新建剪辑会话</button>
        <div className="side-label side-label-row"><span>项目</span><button className="add-project" onClick={() => void createProjectWorkspace()} aria-label="新建项目">+</button></div>
        <nav className="project-list" aria-label="本地项目">
          {projects.map((project) => <button key={project.id} className={`project-card ${project.id === activeProjectId ? 'active' : ''}`} onClick={() => void selectProject(project.id)}><span className="project-dot" /><span><strong>{project.name}</strong><small>本地项目</small></span></button>)}
          {!projects.length && <p className="empty-projects">新建会话即可开始本地项目。</p>}
        </nav>
        <div className="side-label side-label-row"><span>剪辑会话</span><span>{editingSessions.length}</span></div>
        <nav className="conversation-list" aria-label="剪辑会话">
          {editingSessions.map((session) => <button key={session.id} className={`conversation ${session.id === activeEditingSessionId ? 'active' : ''}`} onClick={() => activeProjectId && void selectEditingSession(activeProjectId, session.id)}>
            <span className={`state-dot ${session.state}`} />
            <span><strong>{session.title}</strong><small>{session.preview}</small></span>
            <time>{session.updated}</time>
          </button>)}
        </nav>
        <div className="sidebar-footer"><button onClick={() => setProviderOpen(true)}><span className="provider-dot" /> {customApiStatus.state === 'connected' ? `自定义 API 已连接` : oauthStatus.state === 'connected' ? 'GPT OAuth 已连接' : '模型未连接'}</button><button><span className="gear">o</span> 项目设置</button><span className={`store-state ${storeState}`}>{storeState === 'ready' ? '本地 SQLite 已就绪' : storeState === 'browser' ? '浏览器原型模式' : '本地存储不可用'}</span></div>
      </aside>

      <section className="workspace">
        <header className="topbar"><div className="crumbs">{activeProject?.name ?? '新 local project'} <span>/</span> {activeEditingSession?.title ?? '开始剪辑会话'}</div><div className="top-actions"><span className="saved">{storeState === 'ready' ? 'local project' : '演示模式'}</span>{storyboard && activeView !== 'artifacts' && <button className="outline-button" onClick={() => setActiveView('artifacts')}>查看成果</button>}</div></header>
        <div className="mode-tabs">
          <button className={activeView === 'chat' ? 'selected' : ''} onClick={() => setActiveView('chat')}>Agent</button>
          <button className={activeView === 'assets' ? 'selected' : ''} onClick={() => setActiveView('assets')}>素材 <span>{assetPage.counts.total}</span></button>
          <button className={activeView === 'artifacts' ? 'selected' : ''} onClick={() => setActiveView('artifacts')}>成果 <span>{storyboard?.shots.length ?? 0}</span></button>
          <div className="timeline-state">{timelineState === 'not-created' ? '尚未创建 timeline' : timelineState === 'draft' ? `timeline v${timeline?.versionNumber ?? 1}` : timelineState === 'preview-generating' ? 'preview 生成中' : timelineState === 'preview-ready' ? 'preview 已生成' : timelineState === 'jianying-pending' ? 'Jianying draft 已生成 · 退出 Jianying 后自动注册' : 'Jianying draft 已注册'}</div>
        </div>
        {activeView === 'assets' ? assetWorkspace : <ConversationWorkspace
          activeEditingSession={activeEditingSession ? { id: activeEditingSession.id, conversationId: activeEditingSession.conversationId, title: activeEditingSession.title, brief: activeEditingSession.brief } : undefined}
          activeView={activeView}
          agentEditListenerReady={agentEditListenerReady}
          agentTasks={agentTasks}
          assetCount={assetPage.counts}
          assets={assets.map((asset) => ({ id: asset.id, name: asset.name }))}
          composerNotice={composerNotice}
          deliveryStatus={deliveryStatus}
          input={input}
          isCreatingJianyingDraft={isCreatingJianyingDraft}
          isCreatingTimeline={isCreatingTimeline}
          isGeneratingStoryboard={isGeneratingStoryboard}
          isRenderingPreview={isRenderingPreview}
          isSending={isSending}
          messages={messages}
          onCreateJianyingDelivery={() => void createJianyingDelivery()}
          onCreateTimelineFromStoryboard={() => void createTimelineFromStoryboard()}
          onGenerateStoryboard={() => void generateStoryboard()}
          onInputChange={(value) => { setInput(value); if (composerNotice) setComposerNotice(null) }}
          onOpenStoryboard={() => setActiveView('artifacts')}
          onRenderLocalPreview={() => void renderLocalPreview()}
          onSendMessage={sendMessage}
          onSetActiveView={setActiveView}
          onStoryboardBriefChange={setStoryboardBrief}
          preview={preview}
          previewNonce={previewNonce}
          routeStatusDetail={routeStatusDetail}
          routeStatusText={routeStatusText}
          routeStatusTone={routeStatusTone}
          setInput={setInput}
          storyboard={storyboard}
          storyboardBrief={storyboardBrief}
          storyboardError={storyboardError}
          timeline={timeline}
          timelineVersions={timelineVersions}
          operationLogs={operationLogs}
        />}
      </section>

      {assetPage.counts.analyzing > 0 && <aside className="analysis-activity" aria-live="polite"><header><span className="state-dot working" /><span>正在分析媒体</span><b>{assetPage.counts.analyzing}</b>{assetPage.counts.queued > 0 && <p className="analysis-queue">另 {assetPage.counts.queued} 个排队等待</p>}</header>{analyzingAssets.length > 0 && <ul>{analyzingAssets.slice(0, 3).map((asset) => <li key={asset.id}>{asset.name}</li>)}</ul>}{assetPage.counts.analyzing > analyzingAssets.slice(0, 3).length && <p>另有 {assetPage.counts.analyzing - analyzingAssets.slice(0, 3).length} 个任务正在运行</p>}</aside>}
      {providerOpen && <div className="modal-backdrop" role="dialog" aria-modal="true" aria-label="模型提供商设置"><section className="provider-modal"><button className="close-button" onClick={() => setProviderOpen(false)} aria-label="关闭">x</button><span className="eyebrow">MODEL ACCESS</span><h2>连接 Agent 模型</h2><p>AI 剪辑 MVP 需要此模型连接。项目文件与原始素材保持在本机；仅在理解需求或分析关键帧时发送最小必要数据。API Key 只保存在 Windows 凭据库。</p><div className="provider-option chosen"><span><strong>OpenAI OAuth</strong><small>实验性 OpenCode 兼容流。令牌只存储在 Windows 凭据库，可能随 OpenAI 服务变更失效。</small></span><b>{oauthStatus.state === 'connected' ? '已连接' : '实验性'}</b></div><p className="oauth-status">{oauthStatus.message ?? '尚未连接。'}</p><button className="primary-button modal-button" onClick={() => void connectExperimentalOpenAI()} disabled={oauthStatus.state === 'pending' || oauthStatus.state === 'connected'}>{oauthStatus.state === 'pending' ? '等待浏览器授权' : oauthStatus.state === 'connected' ? 'OAuth 已连接' : '使用 ChatGPT 登录'}</button>{oauthStatus.state === 'connected' && <button className="outline-button modal-button" onClick={() => void disconnectExperimentalOpenAI()}>退出登录</button>}<div className="provider-divider" /><div className="provider-option chosen"><span><strong>自定义 API</strong><small>任何 OpenAI 兼容的托管端点。主 Model 用于 storyboard 与 Agent；可选粗视觉 Model 仅用于批量画面分析。配置后自定义 API 会优先生效。</small></span><b>{customApiStatus.state === 'connected' ? `${customApiStatus.model ?? '已连接'}` : '自定义'}</b></div><p className="oauth-status">{customApiStatus.message ?? '尚未配置。'}{customApiStatus.state === 'connected' && ` 粗视觉：${customApiStatus.coarseVisualModel ?? '使用主 Model'}`}</p><form className="custom-api-form" onSubmit={(event) => { event.preventDefault(); setIsSavingCustomApi(true); void saveCustomConnection(customBaseUrl, customModel, customCoarseVisualModel, customApiKey).then((ok) => { if (ok) { setCustomBaseUrl(''); setCustomModel(''); setCustomCoarseVisualModel(''); setCustomApiKey('') } setIsSavingCustomApi(false) }) }}><label><span>Base URL</span><input value={customBaseUrl} onChange={(event) => setCustomBaseUrl(event.target.value)} placeholder="https://api.example.com/v1" autoComplete="off" /></label><label><span>Model（必填，storyboard 与 Agent）</span><input value={customModel} onChange={(event) => setCustomModel(event.target.value)} placeholder="例如 main-model" autoComplete="off" /></label><label><span>粗视觉 Model（可选）</span><input value={customCoarseVisualModel} onChange={(event) => setCustomCoarseVisualModel(event.target.value)} placeholder="留空则使用主 Model" autoComplete="off" /></label><label><span>API Key</span><input type="password" value={customApiKey} onChange={(event) => setCustomApiKey(event.target.value)} placeholder="sk-..." autoComplete="off" /></label><button className="primary-button modal-button" type="submit" disabled={isSavingCustomApi || !customBaseUrl.trim() || !customModel.trim() || !customApiKey.trim()}>{isSavingCustomApi ? '保存中' : '保存自定义 API'}</button></form>{customApiStatus.state === 'connected' && <button className="outline-button modal-button" onClick={() => void disconnectCustomApi()}>清除自定义 API</button>}<button className="outline-button modal-button" onClick={() => setProviderOpen(false)}>关闭</button></section></div>}
    </main>
  )
}

export default App
