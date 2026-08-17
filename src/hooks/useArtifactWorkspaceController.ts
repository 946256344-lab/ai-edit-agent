// 成果工作区 controller：加载 storyboard/timeline/preview，并发起具名交付命令。
import { useEffect, useRef, useState } from 'react'
import type { Dispatch, RefObject, SetStateAction } from 'react'
import { listen } from '@tauri-apps/api/event'
import {
  confirmStoryboardAndPreview,
  createJianyingDraft,
  createTimelineDraft,
  generateStoryboard,
  getJianyingRegistrationStatus,
  getLatestStoryboard,
  getLatestTimeline,
  listAgentTasks,
  listMessages,
  listOperationLogs,
  listTimelineVersions,
  renderPreview,
} from '../lib/local-store'
import { toMessage } from '../lib/message'
import type {
  AgentEditEvent,
  JianyingRegistrationStatus,
  PreviewResult,
  StoryboardVersion,
  StoredAgentTask,
  StoredOperationLog,
  TimelineVersion,
} from '../lib/local-store'
import type { EditingSessionView, WorkspaceView } from '../components/workspace-types'

export type TimelineState = 'not-created' | 'draft' | 'preview-generating' | 'preview-ready' | 'jianying-pending' | 'jianying'

export type ArtifactSessionSnapshot = {
  storyboard: StoryboardVersion | null
  timeline: TimelineVersion | null
  preview: PreviewResult | null
  timelineState: TimelineState
  operationLogs: StoredOperationLog[]
  timelineVersions: TimelineVersion[]
}

type ArtifactWorkspaceControllerOptions = {
  desktopRuntime: boolean
  projectId: string | null
  sessionId: string | null
  session: EditingSessionView | undefined
  activeProjectRef: RefObject<string | null>
  activeSessionRef: RefObject<string | null>
  setAgentTasks: Dispatch<SetStateAction<StoredAgentTask[]>>
  setMessages: Dispatch<SetStateAction<any[]>>
  appendAgentMessage: (conversationId: string, sessionId: string, content: string) => Promise<void>
  setSessionBrief: (sessionId: string, brief: string) => void
  selectView: Dispatch<SetStateAction<WorkspaceView>>
}

export function getDeliveryStatus(
  storyboard: StoryboardVersion | null,
  timeline: TimelineVersion | null,
  preview: PreviewResult | null,
  timelineState: TimelineState,
) {
  if (!storyboard) return '等待生成故事板'
  if (!timeline) return '故事板已就绪 · 可创建内部时间线'
  if (timelineState === 'preview-generating') return '预览生成中'
  if (timelineState === 'jianying-pending') return '预览已完成 · 等待剪映注册'
  if (timelineState === 'jianying') return '草稿已交付到剪映'
  if (!preview) return '内部时间线已就绪 · 可生成预览'
  return '预览已就绪 · 可交付草稿'
}

export function getTimelineLabel(timelineState: TimelineState, timeline: TimelineVersion | null) {
  if (timelineState === 'not-created') return '尚未创建 timeline'
  if (timelineState === 'draft') return `timeline v${timeline?.versionNumber ?? 1}`
  if (timelineState === 'preview-generating') return 'preview 生成中'
  if (timelineState === 'preview-ready') return 'preview 已生成'
  if (timelineState === 'jianying-pending') return 'Jianying draft 已生成 · 退出 Jianying 后自动注册'
  return 'Jianying draft 已注册'
}

/**
 * Owns the selected editing task's storyboard → timeline → preview → Jianying
 * projection. Every write calls a named Tauri command that creates or delivers
 * a new version; this controller never treats UI state as the artifact source
 * of truth.
 */
export function useArtifactWorkspaceController(options: ArtifactWorkspaceControllerOptions) {
  const [storyboard, setStoryboard] = useState<StoryboardVersion | null>(null)
  const [storyboardBrief, setStoryboardBrief] = useState('')
  const [storyboardError, setStoryboardError] = useState<string | null>(null)
  const [isGeneratingStoryboard, setIsGeneratingStoryboard] = useState(false)
  const [timelineState, setTimelineState] = useState<TimelineState>('not-created')
  const [timeline, setTimeline] = useState<TimelineVersion | null>(null)
  const [preview, setPreview] = useState<PreviewResult | null>(null)
  const [previewNonce, setPreviewNonce] = useState(0)
  const [isCreatingTimeline, setIsCreatingTimeline] = useState(false)
  const [isRenderingPreview, setIsRenderingPreview] = useState(false)
  const [isCreatingJianyingDraft, setIsCreatingJianyingDraft] = useState(false)
  const [operationLogs, setOperationLogs] = useState<StoredOperationLog[]>([])
  const [timelineVersions, setTimelineVersions] = useState<TimelineVersion[]>([])
  const activeTimelineRef = useRef<string | null>(null)

  useEffect(() => {
    activeTimelineRef.current = timeline?.id ?? null
  }, [timeline])

  useEffect(() => {
    if (!options.desktopRuntime) return
    let stopListening: (() => void) | undefined
    void listen<JianyingRegistrationStatus>('jianying-draft-registration-status', (event) => {
      if (event.payload.timelineVersionId !== activeTimelineRef.current) return
      setTimelineState(
        event.payload.status === 'registered'
          ? 'jianying'
          : event.payload.status === 'pending'
            ? 'jianying-pending'
            : 'draft',
      )
    }).then((unlisten) => { stopListening = unlisten })
    return () => stopListening?.()
  }, [options.desktopRuntime])

  function applyPreview(nextPreview: PreviewResult | null) {
    if (nextPreview) setPreviewNonce((nonce) => nonce + 1)
    setPreview(nextPreview)
    setIsRenderingPreview(false)
  }

  function reset() {
    setStoryboard(null)
    setStoryboardBrief('')
    setStoryboardError(null)
    setTimeline(null)
    applyPreview(null)
    setTimelineState('not-created')
    setOperationLogs([])
    setTimelineVersions([])
  }

  function applyAgentResult(result: AgentEditEvent['result']) {
    if (!result) return
    if (result.storyboard) {
      setStoryboard(result.storyboard)
      setStoryboardBrief(result.storyboard.brief)
      setTimeline(null)
      applyPreview(null)
      setTimelineState('not-created')
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
    }
    if (result.preview) applyPreview(result.preview)
  }

  async function loadSession(projectId: string, sessionId: string): Promise<ArtifactSessionSnapshot> {
    const latestStoryboard = await getLatestStoryboard(projectId, sessionId)
    const [latestTimeline, nextOperationLogs, nextTimelineVersions] = await Promise.all([
      latestStoryboard ? getLatestTimeline(projectId, latestStoryboard.id) : Promise.resolve(null),
      listOperationLogs(projectId, sessionId),
      latestStoryboard ? listTimelineVersions(projectId, sessionId, latestStoryboard.id) : Promise.resolve([]),
    ])
    const registration = latestTimeline
      ? await getJianyingRegistrationStatus(latestTimeline.timeline.id)
      : null
    const nextTimelineState: TimelineState = registration?.status === 'pending'
      ? 'jianying-pending'
      : registration?.status === 'registered'
        ? 'jianying'
        : latestTimeline?.preview
          ? 'preview-ready'
          : latestTimeline
            ? 'draft'
            : 'not-created'
    return {
      storyboard: latestStoryboard,
      timeline: latestTimeline?.timeline ?? null,
      preview: latestTimeline?.preview ?? null,
      timelineState: nextTimelineState,
      operationLogs: nextOperationLogs,
      timelineVersions: nextTimelineVersions,
    }
  }

  function applySessionSnapshot(snapshot: ArtifactSessionSnapshot) {
    setStoryboard(snapshot.storyboard)
    setStoryboardBrief(snapshot.storyboard?.brief ?? '')
    setTimeline(snapshot.timeline)
    applyPreview(snapshot.preview)
    setTimelineState(snapshot.timelineState)
    setOperationLogs(snapshot.operationLogs)
    setTimelineVersions(snapshot.timelineVersions)
  }

  async function refreshAudit(
    projectId: string,
    sessionId: string,
    conversationId: string,
    storyboardVersionId: string | null,
  ) {
    const [nextTasks, nextLogs, nextTimelineVersions] = await Promise.all([
      listAgentTasks(projectId, sessionId, conversationId),
      listOperationLogs(projectId, sessionId),
      storyboardVersionId ? listTimelineVersions(projectId, sessionId, storyboardVersionId) : Promise.resolve([]),
    ])
    if (options.activeProjectRef.current === projectId && options.activeSessionRef.current === sessionId) {
      options.setAgentTasks(nextTasks)
      setOperationLogs(nextLogs)
      setTimelineVersions(nextTimelineVersions)
    }
  }

  async function createStoryboard() {
    const brief = storyboardBrief.trim()
    if (!options.projectId || !options.sessionId || isGeneratingStoryboard || !brief) {
      if (!brief) setStoryboardError('请先描述要制作的视频目标、时长、语言和重点。')
      return
    }
    const projectId = options.projectId
    const sessionId = options.sessionId
    setIsGeneratingStoryboard(true)
    setStoryboardError(null)
    try {
      const generated = await generateStoryboard(projectId, sessionId, brief)
      if (options.activeProjectRef.current !== projectId || options.activeSessionRef.current !== sessionId) return
      setStoryboard(generated)
      setTimeline(null)
      applyPreview(null)
      setTimelineState('not-created')
      if (options.session?.conversationId) {
        await options.appendAgentMessage(
          options.session.conversationId,
          sessionId,
          `已根据当前剪辑会话创建故事板 v${generated.versionNumber}。你可以检查镜头，或继续要求创建草稿和预览。`,
        )
      }
      options.setSessionBrief(sessionId, brief)
      options.selectView('artifacts')
    } catch {
      if (options.activeProjectRef.current === projectId && options.activeSessionRef.current === sessionId) {
        setStoryboardError('Agent 未能生成可用 storyboard；没有修改现有版本。请确认素材分析已完成后重试。')
      }
    } finally {
      setIsGeneratingStoryboard(false)
    }
  }

  async function createTimeline() {
    if (!options.projectId || !storyboard || isCreatingTimeline) return
    const projectId = options.projectId
    setIsCreatingTimeline(true)
    try {
      const generatedTimeline = await createTimelineDraft(projectId, storyboard.id)
      if (options.activeProjectRef.current !== projectId) return
      setTimeline(generatedTimeline)
      activeTimelineRef.current = generatedTimeline.id
      applyPreview(null)
      setTimelineState('draft')
      if (options.session?.conversationId) {
        await options.appendAgentMessage(
          options.session.conversationId,
          options.session.id,
          '内部时间线已生成，你现在可以直接生成预览，或继续要求我调整镜头顺序。',
        )
      }
      await refreshAudit(
        projectId,
        options.sessionId ?? '',
        options.session?.conversationId ?? '',
        storyboard.id,
      )
    } finally {
      setIsCreatingTimeline(false)
    }
  }

  async function createPreview() {
    if (!options.projectId || !timeline || isRenderingPreview) return
    const projectId = options.projectId
    setIsRenderingPreview(true)
    setTimelineState('preview-generating')
    try {
      const generatedPreview = await renderPreview(timeline.id)
      if (options.activeProjectRef.current !== projectId) return
      applyPreview(generatedPreview)
      setTimelineState('preview-ready')
      if (options.session?.conversationId) {
        await options.appendAgentMessage(
          options.session.conversationId,
          options.session.id,
          '本地预览已经生成，你可以先检查节奏、镜头和字幕，再决定是否交付到剪映。',
        )
      }
    } finally {
      setIsRenderingPreview(false)
    }
  }

  async function deliverJianyingDraft() {
    if (!options.projectId || !timeline || isCreatingJianyingDraft) return
    const projectId = options.projectId
    setIsCreatingJianyingDraft(true)
    try {
      const draft = await createJianyingDraft(timeline.id)
      if (options.activeProjectRef.current !== projectId) return
      activeTimelineRef.current = timeline.id
      setTimelineState(draft.registrationStatus === 'pending' ? 'jianying-pending' : 'jianying')
      if (options.session?.conversationId) {
        await options.appendAgentMessage(
          options.session.conversationId,
          options.session.id,
          draft.registrationStatus === 'pending'
            ? '剪映草稿已生成，等待退出剪映后自动注册。'
            : '剪映草稿已交付，你可以打开剪映继续查看。',
        )
      }
    } finally {
      setIsCreatingJianyingDraft(false)
    }
  }

  // 返回后台任务 ID，供 App 层注册 pendingEdit 并驱动 reconciliation 轮询。
  async function confirmStoryboard(): Promise<string> {
    if (!options.projectId || !options.sessionId || !options.session || !storyboard) {
      throw new Error('Storyboard confirmation preconditions not met.')
    }
    const conversationId = options.session.conversationId
    if (!conversationId) throw new Error('No active conversation.')
    const projectId = options.projectId
    const sessionId = options.sessionId
    const storyboardId = storyboard.id
    const taskId = await confirmStoryboardAndPreview(projectId, sessionId, conversationId, storyboardId)
    const [artifactSnapshot, nextMessages, nextAgentTasks] = await Promise.all([
      loadSession(projectId, sessionId),
      listMessages(conversationId),
      listAgentTasks(projectId, sessionId, conversationId),
    ])
    if (options.activeProjectRef.current === projectId && options.activeSessionRef.current === sessionId) {
      options.setMessages(nextMessages.map(toMessage))
      options.setAgentTasks(nextAgentTasks)
      applySessionSnapshot(artifactSnapshot)
    }
    return taskId
  }

  return {
    storyboard,
    timeline,
    preview,
    timelineState,
    operationLogs,
    timelineVersions,
    loadSession,
    applySessionSnapshot,
    applyAgentResult,
    refreshAudit,
    reset,
    model: {
      storyboard,
      storyboardBrief,
      storyboardError,
      timeline,
      preview,
      previewNonce,
      deliveryStatus: getDeliveryStatus(storyboard, timeline, preview, timelineState),
      operationLogs,
      timelineVersions,
      busy: {
        generatingStoryboard: isGeneratingStoryboard,
        creatingTimeline: isCreatingTimeline,
        renderingPreview: isRenderingPreview,
        creatingJianyingDraft: isCreatingJianyingDraft,
      },
    },
    actions: {
      setStoryboardBrief,
      generateStoryboard: () => void createStoryboard(),
      createTimeline: () => void createTimeline(),
      renderPreview: () => void createPreview(),
      createJianyingDraft: () => void deliverJianyingDraft(),
      confirmStoryboard: () => confirmStoryboard(),
    },
  }
}

export type ArtifactWorkspaceController = ReturnType<typeof useArtifactWorkspaceController>
