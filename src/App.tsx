import { useEffect, useRef, useState } from 'react'
import type { FormEvent } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { convertFileSrc } from '@tauri-apps/api/core'
import { openUrl } from '@tauri-apps/plugin-opener'
import { listen } from '@tauri-apps/api/event'
import './App.css'
import {
  createConversation as createStoredConversation,
  createEditingSession as createStoredEditingSession,
  executeAgentEdit,
  createMessage as createStoredMessage,
  createProject as createStoredProject,
  getAssetEvidence,
  getJianyingRegistrationStatus,
  getLatestStoryboard,
  getLatestTimeline,
  generateStoryboard as generateStoredStoryboard,
  getExperimentalOpenAIOAuthStatus,
  importAssetFolder as importStoredAssetFolder,
  initializeLocalStore,
  importAssets as importStoredAssets,
  isDesktopRuntime,
  listEditingSessions,
  listAssets,
  listMessages,
  listProjects,
  startExperimentalOpenAIOAuth,
  setConversationStatus,
} from './lib/local-store'
import type { AssetEvidence, ExperimentalOAuthStatus, JianyingRegistrationStatus, PreviewResult, StoryboardVersion, StoredAsset, StoredEditingSession, StoredMessage, StoredProject, TimelineVersion } from './lib/local-store'

type EditingSession = {
  id: string
  conversationId: string | null
  title: string
  preview: string
  brief: string
  updated: string
  state: 'ready' | 'working' | 'review'
}

function errorMessage(error: unknown) {
  if (error instanceof Error) return error.message
  if (typeof error === 'string') return error
  return '未知错误'
}

type Asset = {
  id: string
  name: string
  folderName: string | null
  relativePath: string | null
  kind: 'video' | 'image' | 'audio' | 'other'
  duration: string
  status: 'ready' | 'analyzing' | 'failed'
  tags: string[]
  color: string
  thumbnailUrl: string | null
}

type AssetFolder = {
  id: string
  name: string
  assets: Asset[]
  folders: AssetFolder[]
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

function toAsset(asset: StoredAsset): Asset {
  const kind = asset.kind === 'video' || asset.kind === 'image' || asset.kind === 'audio' ? asset.kind : 'other'
  return {
    id: asset.id,
    name: asset.displayName,
    folderName: asset.folderName,
    relativePath: asset.relativePath,
    kind,
    duration: formatDuration(asset.durationMs),
    status: !asset.sourceAvailable ? 'failed' : asset.analysisStatus === 'ready' ? 'ready' : asset.analysisStatus === 'failed' ? 'failed' : 'analyzing',
    tags: [kind === 'other' ? '其他素材' : kind, asset.width && asset.height ? `${asset.width} x ${asset.height}` : '', asset.fps ? `${asset.fps.toFixed(1)} fps` : '', asset.hasAudio ? '含音频' : '', asset.keyframeCount ? `${asset.keyframeCount} 关键帧` : '', asset.sceneCount ? `${asset.sceneCount} 镜头` : '', asset.ocrTextCount ? `${asset.ocrTextCount} 文本` : '', asset.visualTagCount ? `${asset.visualTagCount} 视觉标签` : ''].filter(Boolean),
    color: kind === 'video' ? 'factory' : kind === 'audio' ? 'audio' : kind === 'image' ? 'product' : 'imported',
    thumbnailUrl: asset.thumbnailPath && isDesktopRuntime() ? convertFileSrc(asset.thumbnailPath) : null,
  }
}

function formatDuration(durationMs: number | null) {
  if (durationMs === null) return ''
  const seconds = Math.floor(durationMs / 1000)
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
}

function buildAssetFolders(assets: Asset[]) {
  const roots: AssetFolder[] = []
  for (const asset of assets) {
    const rootName = asset.folderName ?? '未归类素材'
    const existingRoot = roots.find((item) => item.name === rootName)
    let folder: AssetFolder
    if (existingRoot) {
      folder = existingRoot
    } else {
      folder = { id: rootName, name: rootName, assets: [], folders: [] }
      roots.push(folder)
    }
    const parentFolders = asset.relativePath?.split(/[\\/]/).filter(Boolean).slice(0, -1) ?? []
    for (const name of parentFolders) {
      const existingChild = folder.folders.find((item) => item.name === name)
      let child: AssetFolder
      if (existingChild) {
        child = existingChild
      } else {
        child = { id: `${folder.id}/${name}`, name, assets: [], folders: [] }
        folder.folders.push(child)
      }
      folder = child
    }
    folder.assets.push(asset)
  }
  return roots
}

function countFolderAssets(folder: AssetFolder): number {
  return folder.assets.length + folder.folders.reduce((count, child) => count + countFolderAssets(child), 0)
}

function formatEvidenceTime(timeMs: number | null) {
  if (timeMs === null) return '图片'
  return formatDuration(timeMs)
}

function AssetRow({ asset, onSelect }: { asset: Asset; onSelect: (assetId: string) => void }) {
  const analysisCopy = asset.status === 'ready' ? '技术分析完成' : asset.status === 'failed' ? '无法读取媒体' : '正在后台分析'
  return <button type="button" className="asset" onClick={() => onSelect(asset.id)}><div className={`asset-thumbnail ${asset.color}`}>{asset.thumbnailUrl && <img src={asset.thumbnailUrl} alt="" />}<span>{asset.kind === 'video' ? 'VIDEO' : asset.kind.toUpperCase()}</span>{asset.duration && <time>{asset.duration}</time>}</div><div className="asset-info"><strong>{asset.name}</strong><div>{asset.tags.map((tag) => <span key={tag}>{tag}</span>)}</div><small className={asset.status}>{analysisCopy}</small></div></button>
}

function AssetFolderTree({ folders, expandedFolderIds, onToggle, onSelect }: { folders: AssetFolder[]; expandedFolderIds: Set<string>; onToggle: (folderId: string) => void; onSelect: (assetId: string) => void }) {
  return folders.map((folder) => {
    const expanded = expandedFolderIds.has(folder.id)
    return <section className="asset-folder" key={folder.id}>
      <button className="asset-folder-toggle" onClick={() => onToggle(folder.id)} aria-expanded={expanded}><span>{expanded ? '-' : '+'}</span><strong>{folder.name}</strong><small>{countFolderAssets(folder)}</small></button>
      {expanded && <div className="asset-folder-content"><AssetFolderTree folders={folder.folders} expandedFolderIds={expandedFolderIds} onToggle={onToggle} onSelect={onSelect} />{folder.assets.map((asset) => <AssetRow asset={asset} key={asset.id} onSelect={onSelect} />)}</div>}
    </section>
  })
}

function App() {
  const desktopRuntime = isDesktopRuntime()
  const [projects, setProjects] = useState<StoredProject[]>([])
  const [activeProjectId, setActiveProjectId] = useState<string | null>(null)
  const [editingSessions, setEditingSessions] = useState<EditingSession[]>([])
  const [activeEditingSessionId, setActiveEditingSessionId] = useState<string | null>(null)
  const [assets, setAssets] = useState<Asset[]>([])
  const [expandedFolderIds, setExpandedFolderIds] = useState<Set<string>>(new Set())
  const [assetEvidence, setAssetEvidence] = useState<AssetEvidence | null>(null)
  const [storyboard, setStoryboard] = useState<StoryboardVersion | null>(null)
  const [storyboardBrief, setStoryboardBrief] = useState('')
  const [storyboardError, setStoryboardError] = useState<string | null>(null)
  const [isGeneratingStoryboard, setIsGeneratingStoryboard] = useState(false)
  const [messages, setMessages] = useState<Message[]>([])
  const [activeView, setActiveView] = useState<'chat' | 'storyboard'>('chat')
  const [input, setInput] = useState('')
  const [isSending, setIsSending] = useState(false)
  const [providerOpen, setProviderOpen] = useState(false)
  const [timelineState, setTimelineState] = useState<'not-created' | 'draft' | 'preview-generating' | 'preview-ready' | 'jianying-pending' | 'jianying'>('not-created')
  const [timeline, setTimeline] = useState<TimelineVersion | null>(null)
  const [preview, setPreview] = useState<PreviewResult | null>(null)
  const [storeState, setStoreState] = useState<'browser' | 'ready' | 'unavailable'>(desktopRuntime ? 'unavailable' : 'browser')
  const [oauthStatus, setOAuthStatus] = useState<ExperimentalOAuthStatus>({ state: 'disconnected', message: null, experimental: true })
  const activeProjectRef = useRef<string | null>(null)
  const activeEditingSessionRef = useRef<string | null>(null)
  const activeTimelineRef = useRef<string | null>(null)
  const activeProject = projects.find((project) => project.id === activeProjectId)
  const activeEditingSession = editingSessions.find((session) => session.id === activeEditingSessionId)
  const analyzingAssets = assets.filter((asset) => asset.status === 'analyzing')

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
    activeTimelineRef.current = timeline?.id ?? null
  }, [timeline])

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
    if (!desktopRuntime || !activeProjectId) return
    const projectId = activeProjectId
    const refreshAssets = () => void listAssets(projectId)
      .then((storedAssets) => {
        if (activeProjectRef.current === projectId) setAssets(storedAssets.map(toAsset))
      })
      .catch(() => undefined)
    const intervalId = window.setInterval(refreshAssets, 1500)
    return () => window.clearInterval(intervalId)
  }, [activeProjectId, desktopRuntime])

  async function selectProject(projectId: string) {
    activeProjectRef.current = projectId
    setActiveProjectId(projectId)
    const [storedSessions, storedAssets] = await Promise.all([
      listEditingSessions(projectId),
      listAssets(projectId),
    ])
    if (activeProjectRef.current !== projectId) return
    const nextSessions = storedSessions.map(toEditingSession)
    setEditingSessions(nextSessions)
    setAssets(storedAssets.map(toAsset))
    setExpandedFolderIds(new Set())
    setAssetEvidence(null)
    if (nextSessions[0]) await selectEditingSession(projectId, nextSessions[0].id, nextSessions)
    else {
      setActiveEditingSessionId(null)
      activeEditingSessionRef.current = null
      setMessages([])
      setStoryboard(null)
      setTimeline(null)
      setPreview(null)
      setTimelineState('not-created')
    }
  }

  async function selectEditingSession(projectId: string, sessionId: string, knownSessions = editingSessions) {
    activeEditingSessionRef.current = sessionId
    const session = knownSessions.find((candidate) => candidate.id === sessionId)
    if (!session) return
    const latestStoryboard = await getLatestStoryboard(projectId, sessionId)
    const [latestTimeline, nextMessages] = await Promise.all([
      latestStoryboard ? getLatestTimeline(projectId, latestStoryboard.id) : Promise.resolve(null),
      session.conversationId ? listMessages(session.conversationId) : Promise.resolve([]),
    ])
    const registration = latestTimeline
      ? await getJianyingRegistrationStatus(latestTimeline.timeline.id)
      : null
    if (activeProjectRef.current !== projectId || activeEditingSessionRef.current !== sessionId) return
    setEditingSessions(knownSessions)
    setActiveEditingSessionId(sessionId)
    setMessages(nextMessages.map(toMessage))
    setStoryboard(latestStoryboard)
    setStoryboardBrief(latestStoryboard?.brief ?? '')
    setTimeline(latestTimeline?.timeline ?? null)
    setPreview(latestTimeline?.preview ?? null)
    setTimelineState(
      registration?.status === 'pending'
        ? 'jianying-pending'
        : registration?.status === 'registered'
          ? 'jianying'
          : latestTimeline?.preview
            ? 'preview-ready'
            : latestTimeline
              ? 'draft'
              : 'not-created',
    )
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

  async function refreshEditingSessions(projectId: string) {
    const refreshed = (await listEditingSessions(projectId)).map(toEditingSession)
    if (activeProjectRef.current === projectId) {
      setEditingSessions(refreshed)
    }
  }

  async function appendStoredMessage(conversationId: string, sessionId: string, role: StoredMessage['role'], content: string) {
    const storedMessage = await createStoredMessage(conversationId, role, content)
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
      setAssets((current) => [...imported.map(toAsset), ...current])
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
    const nextAssets = (await listAssets(projectId)).map(toAsset)
    if (activeProjectRef.current === projectId) {
      setAssets(nextAssets)
      setExpandedFolderIds((current) => new Set([...current, ...nextAssets.flatMap((asset) => asset.folderName ? [asset.folderName] : [])]))
    }
    await appendStoredMessage(context.conversationId, context.sessionId, 'agent', `已从文件夹导入 ${imported.length} 个素材。仅支持的媒体文件会加入本地分析队列。`)
    await refreshEditingSessions(projectId)
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
      setPreview(null)
      setTimelineState('not-created')
      if (activeEditingSession?.conversationId) {
        await appendStoredMessage(activeEditingSession.conversationId, sessionId, 'agent', `已根据当前剪辑会话创建故事板 v${generated.versionNumber}。你可以检查镜头，或继续要求创建草稿和预览。`)
      }
      setEditingSessions((current) => current.map((session) => session.id === sessionId ? { ...session, brief } : session))
      setActiveView('storyboard')
    } catch (error) {
      const detail = errorMessage(error)
      if (activeProjectRef.current === projectId && activeEditingSessionRef.current === sessionId) {
        setStoryboardError(`故事板生成失败：${detail}`)
      }
    } finally {
      setIsGeneratingStoryboard(false)
    }
  }

  async function sendMessage(event: FormEvent) {
    event.preventDefault()
    const trimmed = input.trim()
    if (!trimmed || isSending || !desktopRuntime) return
    setIsSending(true)
    let context: { conversationId: string; projectId: string; sessionId: string } | null = null
    try {
      context = await ensureEditingSession()
      const { conversationId, projectId, sessionId } = context
      await appendStoredMessage(conversationId, sessionId, 'user', trimmed)
      await setConversationStatus(conversationId, 'working')
      await refreshEditingSessions(projectId)
      setInput('')
      const result = await executeAgentEdit(projectId, sessionId, storyboard?.id ?? null, timeline?.id ?? null, trimmed)
      if (result.storyboard && activeEditingSessionRef.current === sessionId) {
        setStoryboard(result.storyboard)
        setStoryboardBrief(result.storyboard.brief)
        setTimeline(null)
        setPreview(null)
        setTimelineState('not-created')
      }
      if (result.timeline && activeEditingSessionRef.current === sessionId) {
        const keepsExistingPreview = Boolean(result.jianyingDraft && timeline?.id === result.timeline.id)
        setTimeline(result.timeline)
        if (!keepsExistingPreview) setPreview(result.preview)
        setTimelineState(
          result.jianyingDraft?.registrationStatus === 'pending'
            ? 'jianying-pending'
            : result.jianyingDraft
              ? 'jianying'
              : result.preview
                ? 'preview-ready'
                : 'draft',
        )
      }
      if (result.preview && activeEditingSessionRef.current === sessionId) setPreview(result.preview)
      await appendStoredMessage(conversationId, sessionId, 'agent', result.message)
    } catch (error) {
      const detail = errorMessage(error)
      if (context) {
        try {
          await appendStoredMessage(context.conversationId, context.sessionId, 'agent', `模型无法完成这项受限剪辑操作：${detail}`)
        } catch {
          // The composer still unlocks below when local persistence is unavailable.
        }
      }
    } finally {
      if (context) {
        try {
          await setConversationStatus(context.conversationId, 'ready')
        } catch {
          // A later successful request or application restart can refresh persisted status.
        }
        try {
          await refreshEditingSessions(context.projectId)
        } catch {
          // Keep the current in-memory editing-session list when persistence is unavailable.
        }
      }
      setIsSending(false)
    }
  }

  function toggleAssetFolder(folderId: string) {
    setExpandedFolderIds((current) => {
      const next = new Set(current)
      if (next.has(folderId)) next.delete(folderId)
      else next.add(folderId)
      return next
    })
  }

  if (!desktopRuntime) {
    return <main className="app-shell browser-notice"><section><span className="eyebrow">DESKTOP APP REQUIRED</span><h1>请在 Windows 桌面应用中运行 Assembly Video Agent</h1><p>浏览器模式不能访问本地项目、媒体文件、FFmpeg 或 AI 凭据，因此不能用于剪辑测试。</p><code>npm run tauri:dev</code></section></main>
  }

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
        <div className="sidebar-footer"><button onClick={() => setProviderOpen(true)}><span className="provider-dot" /> {oauthStatus.state === 'connected' ? 'GPT OAuth 已连接' : 'GPT OAuth 未连接'}</button><button><span className="gear">o</span> 项目设置</button><span className={`store-state ${storeState}`}>{storeState === 'ready' ? '本地 SQLite 已就绪' : storeState === 'browser' ? '浏览器原型模式' : '本地存储不可用'}</span></div>
      </aside>

      <section className="workspace">
        <header className="topbar"><div className="crumbs">{activeProject?.name ?? '新本地项目'} <span>/</span> {activeEditingSession?.title ?? '开始剪辑会话'}</div><div className="top-actions"><span className="saved">{storeState === 'ready' ? '本地项目' : '演示模式'}</span>{storyboard && <button className="outline-button" onClick={() => setActiveView('storyboard')}>查看故事板</button>}</div></header>
        <div className="mode-tabs"><button className={activeView === 'chat' ? 'selected' : ''} onClick={() => setActiveView('chat')}>Agent 对话</button><button className={activeView === 'storyboard' ? 'selected' : ''} onClick={() => setActiveView('storyboard')}>故事板 <span>{storyboard?.shots.length ?? 0}</span></button><div className="timeline-state">{timelineState === 'not-created' ? '尚未创建内部时间线' : timelineState === 'draft' ? `内部时间线 v${timeline?.versionNumber ?? 1}` : timelineState === 'preview-generating' ? '预览生成中' : timelineState === 'preview-ready' ? '预览已生成' : timelineState === 'jianying-pending' ? '剪映草稿已生成 · 退出剪映后自动注册' : '剪映草稿已注册 · 打开剪映查看'}</div></div>
        {activeView === 'chat' ? <section className="conversation-workspace">
          <div className="message-stream">
            <div className="session-intro"><span>当前剪辑会话</span><strong>{storyboard?.title ?? activeEditingSession?.title ?? '从一句话开始剪辑'}</strong><p>{storyboard?.summary ?? activeEditingSession?.brief ?? '描述你想做的视频。Agent 会记录需求、分析本地素材，并将故事板、内部时间线、剪映草稿和预览作为可检查的工具结果。'}</p></div>
            {!messages.length && <div className="empty-chat"><button onClick={() => setInput('制作一条 30 秒的英文产品宣传片')}>制作 30 秒宣传片</button><button onClick={() => void importAssets()}>导入本地素材</button><button onClick={() => setInput('我应该先准备哪些素材？')}>我应该先准备什么？</button></div>}
            {messages.map((message) => <article key={message.id} className={`message ${message.role}`}><div className="message-avatar">{message.role === 'agent' ? 'A' : 'Y'}</div><div className="message-content"><div className="message-meta">{message.role === 'agent' ? 'Assembly Agent' : '你'} <time>{message.time}</time></div><p>{message.content}</p></div></article>)}
            {(assets.length > 0 || storyboard || preview) && <section className="plan-card"><div className="plan-heading"><span>项目上下文</span><button onClick={() => setActiveView('storyboard')}>{storyboard ? '查看故事板' : '需求已记录'}</button></div><ol><li className={assets.length ? 'done' : 'current'}>本地素材 <small>{assets.length ? `${assets.length} 个素材已加入项目` : '等待导入'}</small></li><li className={storyboard ? 'done' : ''}>故事板 <small>{storyboard ? `草案 v${storyboard.versionNumber}` : '直接告诉 Agent 生成'}</small></li><li className={timeline ? 'done' : ''}>内部时间线 <small>{timeline ? `v${timeline.versionNumber} · 仅本应用` : '等待 Agent 创建'}</small></li><li className={preview ? 'done' : ''}>预览 <small>{preview ? '已生成并完成检查' : '等待 Agent 生成'}</small></li></ol></section>}{preview && <section className="preview-card"><span className="eyebrow">LOCAL LOW-RES PREVIEW</span><video controls src={convertFileSrc(preview.previewPath)} />{preview.qualityReport.checks.length > 0 && <div className="quality-checks">{preview.qualityReport.checks.map((check, index) => <p key={`${check.category}-${index}`} className={check.severity}>{check.message}{check.shotIndices.length > 0 ? ` 镜头：${check.shotIndices.join('、')}` : ''}</p>)}</div>}</section>}
          </div>
          <form className="composer" onSubmit={sendMessage}><textarea value={input} onChange={(event) => setInput(event.target.value)} placeholder="描述目标、提问或下达剪辑指令..." rows={2} /><div><span>{activeEditingSession ? `当前会话：${activeEditingSession.title}` : '首次发送将创建本地项目和剪辑会话'}</span><button className="send-button" type="submit" disabled={isSending}>{isSending ? '处理中' : '发送'}</button></div></form>
        </section> : <section className="storyboard-view">{storyboard ? <><div className="storyboard-heading"><div><span className="eyebrow">草案 v{storyboard.versionNumber} · 9:16 · English</span><h1>{storyboard.title}</h1></div><p>{storyboard.summary}</p></div><div className="shot-grid">{storyboard.shots.map((shot) => <article className="shot-card" key={shot.orderIndex}><div className={`shot-image shot-${String(shot.orderIndex).padStart(2, '0')}`}><span>{String(shot.orderIndex).padStart(2, '0')}</span><time>{formatEvidenceTime(shot.durationMs)}</time></div><div className="shot-copy"><strong>{shot.purpose}</strong><p>{assets.find((asset) => asset.id === shot.assetId)?.name ?? '已验证素材'} <span>{formatEvidenceTime(shot.sourceStartMs)} - {formatEvidenceTime(shot.sourceEndMs)}</span></p><small>{shot.reason}</small><em>{shot.onScreenText}</em></div><button onClick={() => { setActiveView('chat'); setInput(`调整第 ${shot.orderIndex} 个镜头：`) }}>调整镜头</button></article>)}</div></> : <div className="empty-storyboard"><span className="eyebrow">EVIDENCE-BASED STORYBOARD</span><h1>先告诉 Agent 要做什么</h1><p>例如：用这些素材制作一条 30 秒英文产品宣传视频，突出工厂实力、产品质量和交付能力。</p><textarea className="brief-input" value={storyboardBrief} onChange={(event) => setStoryboardBrief(event.target.value)} placeholder="描述视频目标、时长、语言、受众和重点信息" rows={5} />{storyboardError && <p className="storyboard-error">{storyboardError}</p>}<button className="primary-button" onClick={() => void generateStoryboard()} disabled={isGeneratingStoryboard || !storyboardBrief.trim()}>{isGeneratingStoryboard ? '正在生成' : '基于该需求生成故事板'}</button></div>}</section>}
      </section>

      <aside className="asset-panel"><header><div><span className="panel-kicker">素材库</span><strong>{assets.length} 个素材</strong></div><div className="import-actions"><button className="import-button" onClick={() => void importAssets()} disabled={!activeProjectId || storeState !== 'ready'}>导入文件</button><button className="import-button" onClick={() => void importAssetFolder()} disabled={!activeProjectId || storeState !== 'ready'}>导入文件夹</button></div></header><div className="asset-filter"><button className="active">全部</button><button>视频</button><button>图片</button><button>音频</button></div><div className="asset-list"><AssetFolderTree folders={buildAssetFolders(assets)} expandedFolderIds={expandedFolderIds} onToggle={toggleAssetFolder} onSelect={(assetId) => void selectAssetEvidence(assetId)} />{!assets.length && <p className="empty-assets">选择本地视频、图片或音频后，Agent 才会开始分析。</p>}</div>{assetEvidence && <section className="evidence-panel"><header><div><span className="panel-kicker">画面证据</span><strong>{assetEvidence.displayName}</strong></div><button className="close-button" onClick={() => setAssetEvidence(null)} aria-label="关闭">x</button></header>{assetEvidence.keyframes.length > 0 && <div className="evidence-frames">{assetEvidence.keyframes.map((frame) => <figure key={frame.imagePath}><img src={convertFileSrc(frame.imagePath)} alt="" /><figcaption>{formatEvidenceTime(frame.timeMs)}</figcaption></figure>)}</div>}{assetEvidence.ocrEvidence.map((evidence) => <p className="evidence-item" key={`${evidence.timeMs}-${evidence.text}`}><span>OCR {formatEvidenceTime(evidence.timeMs)}</span>{evidence.text}</p>)}{assetEvidence.visualEvidence.map((evidence, index) => <p className="evidence-item" key={`${evidence.timeMs}-${index}`}><span>视觉 {formatEvidenceTime(evidence.timeMs)}</span>{[...evidence.subjects, evidence.scene ?? '', ...evidence.actions, ...evidence.products].filter(Boolean).join(' · ') || '未返回可用视觉标签'}</p>)}</section>}<footer className="analysis-note"><span className="state-dot working" /><p>素材仅记录原始文件引用，浏览素材不会修改项目内容。</p></footer></aside>

      {analyzingAssets.length > 0 && <aside className="analysis-activity" aria-live="polite"><header><span className="state-dot working" /><span>正在分析媒体</span><b>{analyzingAssets.length}</b></header><ul>{analyzingAssets.slice(0, 3).map((asset) => <li key={asset.id}>{asset.name}</li>)}</ul>{analyzingAssets.length > 3 && <p>另有 {analyzingAssets.length - 3} 个任务等待完成</p>}</aside>}
      {providerOpen && <div className="modal-backdrop" role="dialog" aria-modal="true" aria-label="模型提供商设置"><section className="provider-modal"><button className="close-button" onClick={() => setProviderOpen(false)} aria-label="关闭">x</button><span className="eyebrow">MODEL ACCESS</span><h2>连接 Agent 模型</h2><p>AI 剪辑 MVP 需要此模型连接。项目文件与原始素材保持在本机；仅在理解需求或分析关键帧时发送最小必要数据。</p><div className="provider-option chosen"><span><strong>OpenAI OAuth</strong><small>实验性 OpenCode 兼容流。令牌只存储在 Windows 凭据库，可能随 OpenAI 服务变更失效。</small></span><b>{oauthStatus.state === 'connected' ? '已连接' : '实验性'}</b></div><p className="oauth-status">{oauthStatus.message ?? '尚未连接。'}</p><button className="primary-button modal-button" onClick={() => void connectExperimentalOpenAI()} disabled={oauthStatus.state === 'pending' || oauthStatus.state === 'connected'}>{oauthStatus.state === 'pending' ? '等待浏览器授权' : oauthStatus.state === 'connected' ? 'OAuth 已连接' : '使用 ChatGPT 登录'}</button><button className="outline-button modal-button" onClick={() => setProviderOpen(false)}>关闭</button></section></div>}
    </main>
  )
}

export default App
