// 成果工作区 controller：加载 storyboard/timeline/preview，并发起具名交付命令。
import { useEffect, useRef, useState } from 'react'
import type { Dispatch, RefObject, SetStateAction } from 'react'
import { listen } from '@tauri-apps/api/event'
import {
  createJianyingDraft,
  createTimelineDraft,
  generateStoryboard,
  getJianyingRegistrationStatus,
  getLatestStoryboard,
  getLatestTimeline,
  listAgentTasks,
  listOperationLogs,
  listTimelineVersions,
  renderPreview,
  synthesizeStoryboardVoiceover,
} from '../lib/local-store'
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
  if (!storyboard) return '等待开始剪辑'
  if (!timeline) return '第一版镜头已选好'
  if (timelineState === 'preview-generating') return '正在生成预览'
  if (timelineState === 'jianying-pending') return '预览已完成 · 草稿待剪映注册'
  if (timelineState === 'jianying') return '剪映草稿已就绪'
  if (!preview) return '剪辑已完成 · 等待预览'
  return '预览已就绪'
}

export function getTimelineLabel(timelineState: TimelineState, timeline: TimelineVersion | null) {
  if (timelineState === 'not-created') return '尚未创建 timeline'
  if (timelineState === 'draft') return `timeline v${timeline?.versionNumber ?? 1}`
  if (timelineState === 'preview-generating') return 'preview 生成中'
  if (timelineState === 'preview-ready') return 'preview 已生成'
  if (timelineState === 'jianying-pending') return 'Jianying draft 已生成 · 退出 Jianying 后自动注册'
  return 'Jianying draft 已注册'
}

function draftNameFromResult(draftDirectory: string) {
  const parts = draftDirectory.split(/[/\\]/).filter(Boolean)
  return parts[parts.length - 1] || '剪映草稿'
}

function jianyingDeliverErrorMessage(error: unknown) {
  const raw = error instanceof Error ? error.message : String(error ?? '')
  if (/draft library is unavailable/i.test(raw)) {
    return '找不到剪映草稿库。请先打开一次剪映并新建任意本地草稿，然后再试。'
  }
  if (/not yet verified for Jianying/i.test(raw)) {
    return '当前字幕样式还不支持交付剪映。请先用本地预览确认，或去掉未验证的字幕效果后再试。'
  }
  if (/Python with pyJianYingDraft is unavailable/i.test(raw)) {
    return '本机缺少 Python（py）或 pyJianYingDraft，无法生成剪映草稿。'
  }
  if (/source media|unavailable asset|Music source/i.test(raw)) {
    return '有素材文件找不到了。请先在素材页重新定位缺失文件，再交付剪映。'
  }
  if (/Jianying Pro is still running/i.test(raw)) {
    return '剪映仍在运行，草稿注册未完成。请完全退出剪映后，再回到这里点一次。'
  }
  return '剪映草稿未能生成。请确认已安装剪映专业版，并完全退出剪映后重试。'
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
  const [jianyingNotice, setJianyingNotice] = useState<string | null>(null)
  const [jianyingNoticeTone, setJianyingNoticeTone] = useState<'info' | 'error'>('info')
  const [operationLogs, setOperationLogs] = useState<StoredOperationLog[]>([])
  const [timelineVersions, setTimelineVersions] = useState<TimelineVersion[]>([])
  const activeTimelineRef = useRef<string | null>(null)
  const snapshotSessionRef = useRef<string | null>(null)

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
    snapshotSessionRef.current = null
    activeTimelineRef.current = null
    setStoryboard(null)
    setStoryboardBrief('')
    setStoryboardError(null)
    setTimeline(null)
    applyPreview(null)
    setTimelineState('not-created')
    setOperationLogs([])
    setTimelineVersions([])
    setJianyingNotice(null)
  }

  function applyAgentResult(result: AgentEditEvent['result']) {
    if (!result) return
    if (result.storyboard) {
      setStoryboard(result.storyboard)
      setStoryboardBrief(result.storyboard.brief)
      setTimeline(null)
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
    activeTimelineRef.current = snapshot.timeline?.id ?? null
    if (snapshot.preview || snapshotSessionRef.current !== options.activeSessionRef.current) applyPreview(snapshot.preview)
    snapshotSessionRef.current = options.activeSessionRef.current
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
          `已根据当前需求生成镜头方案 v${generated.versionNumber}。系统会继续尝试生成剪辑和预览。`,
        )
      }
      options.setSessionBrief(sessionId, brief)
      options.selectView('artifacts')
      const nextTimeline = await createTimelineDraft(projectId, generated.id)
      if (options.activeProjectRef.current !== projectId || options.activeSessionRef.current !== sessionId) return
      setTimeline(nextTimeline)
      setTimelineState('draft')
      if (options.session?.conversationId) {
        await options.appendAgentMessage(
          options.session.conversationId,
          sessionId,
          `剪辑 v${nextTimeline.versionNumber} 已生成，继续生成预览。`,
        )
      }
      // 自动合成配音+对齐字幕：失败（例如未配置 ElevenLabs）时跳过，
      // 仍用当前 timeline 渲染预览，不阻塞主流程。
      let previewTimeline = nextTimeline
      const conversationId = options.session?.conversationId
      if (conversationId) {
        try {
          const voiced = await synthesizeStoryboardVoiceover(
            projectId,
            sessionId,
            conversationId,
            nextTimeline.id,
          )
          if (options.activeProjectRef.current !== projectId || options.activeSessionRef.current !== sessionId) return
          const latest = await getLatestTimeline(projectId, generated.id)
          if (latest?.timeline) {
            previewTimeline = latest.timeline
            setTimeline(latest.timeline)
          }
          await options.appendAgentMessage(
            conversationId,
            sessionId,
            voiced.subtitleApplied
              ? `已自动合成配音（${voiced.provider}）并写入对齐字幕（cue=${voiced.subtitleCueCount}）。`
              : `已自动合成配音（${voiced.provider}）；对齐字幕未写入，旁白已保留。`,
          )
        } catch (error) {
          const detail = error instanceof Error ? error.message : String(error)
          console.warn(`Automatic voiceover skipped: ${detail}`)
          // 无旁白文案 / 已有轨：不提示「配置不可用」
          const silent =
            /no narration text|already has voiceover|narration is missing/i.test(detail)
          if (!silent) {
            const pictureTooShort = /voiceover_longer_than_picture/i.test(detail)
            const brief = detail.length > 160 ? `${detail.slice(0, 160)}…` : detail
            await options.appendAgentMessage(
              conversationId,
              sessionId,
              pictureTooShort
                ? '自动配音未写入：旁白长于画面（禁止冻帧）。请先补足画面时长后再配音；预览暂不含配音。'
                : `自动配音未写入：${brief}。预览将不包含配音。`,
            )
          }
        }
      }
      const previewResult = await renderPreview(previewTimeline.id)
      if (options.activeProjectRef.current !== projectId || options.activeSessionRef.current !== sessionId) return
      applyPreview(previewResult)
      setTimelineState('preview-ready')
      if (options.session?.conversationId) {
        await options.appendAgentMessage(
          options.session.conversationId,
          sessionId,
          '本地预览已经生成，可以直接查看。',
        )
      }
    } catch {
      if (options.activeProjectRef.current === projectId && options.activeSessionRef.current === sessionId) {
        setStoryboardError('没能生成可用镜头方案；没有修改现有版本。请确认素材分析已完成后重试。')
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
          '剪辑已生成，你现在可以直接生成预览，或继续要求我调整镜头顺序。',
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
      if (options.activeProjectRef.current !== projectId || options.activeSessionRef.current !== options.sessionId || activeTimelineRef.current !== timeline.id) return
      applyPreview(generatedPreview)
      setTimelineState('preview-ready')
      if (options.session?.conversationId) {
        await options.appendAgentMessage(
          options.session.conversationId,
          options.session.id,
          '本地预览已经生成，你可以先检查节奏、镜头和字幕，再决定是否交付到剪映。',
        )
      }
    } catch {
      if (activeTimelineRef.current === timeline.id && options.activeSessionRef.current === options.sessionId) {
        setTimelineState('draft')
        setJianyingNotice('预览未能生成，已保存的粗剪仍保留。请检查素材是否可用后再生成预览。')
      }
    } finally {
      setIsRenderingPreview(false)
    }
  }

  async function deliverJianyingDraft(override?: TimelineVersion) {
    const deliveryTimeline = override ?? timeline
    if (!options.projectId || !deliveryTimeline || isCreatingJianyingDraft) {
      if (!timeline) {
        setJianyingNoticeTone('error')
        setJianyingNotice('还没有可交付的剪辑结果。请先在 Agent 里生成预览，或在下方详情里创建时间线。')
      }
      return
    }
    const projectId = options.projectId
    setIsCreatingJianyingDraft(true)
    setJianyingNotice(null)
    try {
      const draft = await createJianyingDraft(deliveryTimeline.id)
      if (options.activeProjectRef.current !== projectId || options.activeSessionRef.current !== options.sessionId || activeTimelineRef.current !== deliveryTimeline.id) return
      const draftName = draftNameFromResult(draft.draftDirectory)
      const pending = draft.registrationStatus === 'pending'
      setTimelineState(pending ? 'jianying-pending' : 'jianying')
      setJianyingNoticeTone('info')
      setJianyingNotice(
        pending
          ? `草稿「${draftName}」已写好，但剪映正在运行，列表还不会刷新。请完全退出剪映后再打开，即可看到。`
          : `草稿「${draftName}」已生成并注册。请在剪映本地草稿箱中查找该名称；若剪映已打开，请重启后再看。`,
      )
      if (options.session?.conversationId) {
        await options.appendAgentMessage(
          options.session.conversationId,
          options.session.id,
          pending
            ? `剪映草稿「${draftName}」已生成，等待退出剪映后自动注册。`
            : `剪映草稿「${draftName}」已交付，可在剪映本地草稿中打开。`,
        )
      }
    } catch (error) {
      setJianyingNoticeTone('error')
      setJianyingNotice(jianyingDeliverErrorMessage(error))
    } finally {
      setIsCreatingJianyingDraft(false)
    }
  }

  function applyStudioCommit(nextTimeline: TimelineVersion, nextPreview: PreviewResult | null) {
    setTimeline(nextTimeline)
    activeTimelineRef.current = nextTimeline.id
    if (nextPreview) {
      applyPreview(nextPreview)
      setTimelineState('preview-ready')
    } else {
      setJianyingNotice(null)
      setTimelineState('draft')
    }
    setTimelineVersions((prev) => [nextTimeline, ...prev.filter((v) => v.id !== nextTimeline.id)])
  }

  // 返回后台任务 ID，供 App 层注册 pendingEdit 并驱动 reconciliation 轮询。
  return {
    storyboard,
    timeline,
    preview,
    previewNonce,
    timelineState,
    operationLogs,
    timelineVersions,
    loadSession,
    applySessionSnapshot,
    applyAgentResult,
    applyStudioCommit,
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
      jianyingNotice,
      jianyingNoticeTone,
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
      createJianyingDraft: (override?: TimelineVersion) => void deliverJianyingDraft(override),
    },
  }
}

export type ArtifactWorkspaceController = ReturnType<typeof useArtifactWorkspaceController>
