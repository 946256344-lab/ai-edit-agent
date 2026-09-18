// 产物工作区 controller：加载 storyboard/timeline/preview，并按所选输出端口交付。
import { useEffect, useRef, useState } from 'react'
import type { Dispatch, RefObject, SetStateAction } from 'react'
import { listen } from '@tauri-apps/api/event'
import {
  createTimelineDraft,
  deliverToEditor,
  getJianyingRegistrationStatus,
  getLatestTimeline,
  getAssetEvidence,
  getStoryboardVersion,
  listAgentTasks,
  listEditorLinkers,
  listStoryboardVersions,
  renderPreview,
  setOutputEditor,
} from '../lib/local-store'
import type {
  AgentEditEvent,
  EditorLinkerCatalog,
  JianyingRegistrationStatus,
  PreviewResult,
  StoryboardVersion,
  StoredAgentTask,
  TimelineVersion,
} from '../lib/local-store'
import type { EditingSessionView } from '../components/workspace-types'

export type TimelineState = 'not-created' | 'draft' | 'preview-generating' | 'preview-ready' | 'jianying-pending' | 'jianying' | 'exported'

export type ArtifactSessionSnapshot = {
  storyboard: StoryboardVersion | null
  storyboardVersions: StoryboardVersion[]
  timeline: TimelineVersion | null
  preview: PreviewResult | null
  timelineState: TimelineState
}

type ArtifactWorkspaceControllerOptions = {
  desktopRuntime: boolean
  projectId: string | null
  sessionId: string | null
  session: EditingSessionView | undefined
  activeProjectRef: RefObject<string | null>
  activeSessionRef: RefObject<string | null>
  setAgentTasks: Dispatch<SetStateAction<StoredAgentTask[]>>
  appendAgentMessage: (conversationId: string, sessionId: string, content: string) => Promise<void>
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
  if (timelineState === 'exported') return '编辑器文件已导出'
  if (!preview) return '剪辑已完成 · 等待预览'
  return '预览已就绪'
}

function deliverErrorMessage(error: unknown, editorLabel: string) {
  const raw = error instanceof Error ? error.message : String(error ?? '')
  if (/尚未实现/.test(raw)) return raw
  if (/draft library is unavailable/i.test(raw)) {
    return '找不到剪映草稿库。请先打开一次剪映并新建任意本地草稿，然后再试。'
  }
  if (/not yet verified for Jianying/i.test(raw)) {
    return '当前字幕样式还不支持交付剪映。请先用本地预览确认，或去掉未验证的字幕效果后再试。'
  }
  if (/Python with pyJianYingDraft is unavailable/i.test(raw)) {
    return '本机缺少 Python（py）或 pyJianYingDraft，无法生成剪映草稿。'
  }
  if (/source media|unavailable asset|Music source|Voiceover media/i.test(raw)) {
    return `有素材文件找不到了。请先在素材页重新定位缺失文件，再交付到${editorLabel}。`
  }
  if (/Jianying Pro is still running/i.test(raw)) {
    return '剪映仍在运行，草稿注册未完成。请完全退出剪映后，再回到这里点一次。'
  }
  if (/无法写出|无法准备/.test(raw)) {
    return `没能写出${editorLabel}文件。请确认本机数据目录可写后重试。`
  }
  return `${editorLabel}未能交付。请确认编辑器已安装或改用其他输出端口后重试。`
}

export function deliverActionLabel(editorId: string, busy: boolean) {
  if (busy) return '正在交付…'
  if (editorId === 'fcpxml') return '导出 FCPXML'
  if (editorId === 'otio') return '导出 OTIO'
  if (editorId === 'capcut') return '生成 CapCut 草稿'
  return '生成剪映草稿'
}

/**
 * Owns the selected editing task's storyboard → timeline → preview → editor
 * delivery. Every write calls a named Tauri command that creates or delivers
 * a new version; this controller never treats UI state as the artifact source
 * of truth.
 */
export function useArtifactWorkspaceController(options: ArtifactWorkspaceControllerOptions) {
  const [storyboard, setStoryboard] = useState<StoryboardVersion | null>(null)
  const [timelineState, setTimelineState] = useState<TimelineState>('not-created')
  const [timeline, setTimeline] = useState<TimelineVersion | null>(null)
  const [shotImages, setShotImages] = useState<{ timelineId: string; images: Record<number, { imagePath: string; displayName: string }>; error: string | null } | null>(null)
  const [preview, setPreview] = useState<PreviewResult | null>(null)
  const [previewNonce, setPreviewNonce] = useState(0)
  const [isCreatingTimeline, setIsCreatingTimeline] = useState(false)
  const [isRenderingPreview, setIsRenderingPreview] = useState(false)
  const [isDelivering, setIsDelivering] = useState(false)
  const [deliveryNotice, setDeliveryNotice] = useState<string | null>(null)
  const [deliveryNoticeTone, setDeliveryNoticeTone] = useState<'info' | 'error'>('info')
  const [editorCatalog, setEditorCatalog] = useState<EditorLinkerCatalog>({
    selectedId: 'jianying',
    linkers: [],
  })
  const [storyboardVersions, setStoryboardVersions] = useState<StoryboardVersion[]>([])
  const activeTimelineRef = useRef<string | null>(null)
  const snapshotSessionRef = useRef<string | null>(null)
  const openedStoryboardIdsRef = useRef<Map<string, string>>(new Map())

  // 镜头条按时间线读取源区间内关键帧，不依赖素材库当前目录和分页。
  useEffect(() => {
    if (!timeline) return
    let active = true
    void Promise.all([...new Set(timeline.clips.map((clip) => clip.assetId))].map(getAssetEvidence))
      .then((evidence) => {
        const byAsset = new Map(evidence.map((asset) => [asset.id, asset]))
        const images: Record<number, { imagePath: string; displayName: string }> = {}
        for (const clip of timeline.clips) {
          const asset = byAsset.get(clip.assetId)
          const frames = [...(asset?.keyframes ?? []), ...(asset?.segments?.flatMap((segment) => segment.frames) ?? [])]
            .filter((frame) => frame.timeMs >= clip.sourceStartMs && frame.timeMs < clip.sourceEndMs)
          const midpoint = (clip.sourceStartMs + clip.sourceEndMs) / 2
          frames.sort((a, b) => Math.abs(a.timeMs - midpoint) - Math.abs(b.timeMs - midpoint))
          if (asset && frames[0]) images[clip.shotIndex] = { imagePath: frames[0].imagePath, displayName: asset.displayName }
        }
        if (active) setShotImages({ timelineId: timeline.id, images, error: null })
      })
      .catch(() => { if (active) setShotImages({ timelineId: timeline.id, images: {}, error: '镜头缩略图未能读取，仍可按序号选择镜头。' }) })
    return () => { active = false }
  }, [timeline])

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

  useEffect(() => {
    if (!options.desktopRuntime || !options.projectId) return
    let active = true
    void listEditorLinkers(options.projectId)
      .then((catalog) => {
        if (active) setEditorCatalog(catalog)
      })
      .catch(() => {
        if (active) {
          setEditorCatalog({ selectedId: 'jianying', linkers: [] })
        }
      })
    return () => {
      active = false
    }
  }, [options.desktopRuntime, options.projectId])

  function applyPreview(nextPreview: PreviewResult | null) {
    if (nextPreview) setPreviewNonce((nonce) => nonce + 1)
    setPreview(nextPreview)
    setIsRenderingPreview(false)
  }

  function reset() {
    snapshotSessionRef.current = null
    activeTimelineRef.current = null
    openedStoryboardIdsRef.current.clear()
    setStoryboard(null)
    setStoryboardVersions([])
    setTimeline(null)
    applyPreview(null)
    setTimelineState('not-created')
    setDeliveryNotice(null)
  }

  function applyAgentResult(result: AgentEditEvent['result']) {
    if (!result) return
    if (result.storyboard) {
      setStoryboard(result.storyboard)
      setStoryboardVersions((current) => [
        result.storyboard!,
        ...current.filter((version) => version.id !== result.storyboard?.id),
      ])
      if (options.sessionId) {
        openedStoryboardIdsRef.current.set(options.sessionId, result.storyboard.id)
      }
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
    const versions = await listStoryboardVersions(projectId, sessionId)
    const remembered = openedStoryboardIdsRef.current.get(sessionId)
    const opened = versions.find((version) => version.id === remembered) ?? versions[0] ?? null
    if (opened) openedStoryboardIdsRef.current.set(sessionId, opened.id)
    const latestTimeline = opened ? await getLatestTimeline(projectId, opened.id) : null
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
      storyboard: opened,
      storyboardVersions: versions,
      timeline: latestTimeline?.timeline ?? null,
      preview: latestTimeline?.preview ?? null,
      timelineState: nextTimelineState,
    }
  }

  function applySessionSnapshot(snapshot: ArtifactSessionSnapshot) {
    setStoryboard(snapshot.storyboard)
    setStoryboardVersions(snapshot.storyboardVersions)
    setTimeline(snapshot.timeline)
    activeTimelineRef.current = snapshot.timeline?.id ?? null
    if (snapshot.preview || snapshotSessionRef.current !== options.activeSessionRef.current) applyPreview(snapshot.preview)
    snapshotSessionRef.current = options.activeSessionRef.current
    setTimelineState(snapshot.timelineState)
  }

  async function openStoryboard(storyboardVersionId: string) {
    if (!options.projectId || !options.sessionId) return
    const projectId = options.projectId
    const sessionId = options.sessionId
    const selected =
      storyboardVersions.find((version) => version.id === storyboardVersionId) ??
      (await getStoryboardVersion(projectId, sessionId, storyboardVersionId))
    if (options.activeProjectRef.current !== projectId || options.activeSessionRef.current !== sessionId) return
    openedStoryboardIdsRef.current.set(sessionId, selected.id)
    setStoryboard(selected)
    const latestTimeline = await getLatestTimeline(projectId, selected.id)
    if (options.activeProjectRef.current !== projectId || options.activeSessionRef.current !== sessionId) return
    const registration = latestTimeline
      ? await getJianyingRegistrationStatus(latestTimeline.timeline.id)
      : null
    setTimeline(latestTimeline?.timeline ?? null)
    activeTimelineRef.current = latestTimeline?.timeline.id ?? null
    applyPreview(latestTimeline?.preview ?? null)
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

  async function refreshAudit(
    projectId: string,
    sessionId: string,
    conversationId: string,
  ) {
    const nextTasks = await listAgentTasks(projectId, sessionId, conversationId)
    if (options.activeProjectRef.current === projectId && options.activeSessionRef.current === sessionId) {
      options.setAgentTasks(nextTasks)
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
          '本地预览已经生成，你可以先检查节奏、镜头和字幕，再决定是否交付到所选编辑器。',
        )
      }
    } catch {
      if (activeTimelineRef.current === timeline.id && options.activeSessionRef.current === options.sessionId) {
        setTimelineState('draft')
        setDeliveryNotice('预览未能生成，已保存的粗剪仍保留。请检查素材是否可用后再生成预览。')
      }
    } finally {
      setIsRenderingPreview(false)
    }
  }

  async function deliverSelectedEditor(override?: TimelineVersion) {
    const deliveryTimeline = override ?? timeline
    const selected = editorCatalog.linkers.find((linker) => linker.id === editorCatalog.selectedId)
    const editorLabel = selected?.label ?? '编辑器'
    if (!options.projectId || !deliveryTimeline || isDelivering) {
      if (!timeline) {
        setDeliveryNoticeTone('error')
        setDeliveryNotice('还没有可交付的剪辑结果。请先在 Agent 里生成预览，或先创建时间线。')
      }
      return
    }
    const projectId = options.projectId
    setIsDelivering(true)
    setDeliveryNotice(null)
    try {
      const delivery = await deliverToEditor(deliveryTimeline.id, editorCatalog.selectedId)
      if (options.activeProjectRef.current !== projectId || options.activeSessionRef.current !== options.sessionId || activeTimelineRef.current !== deliveryTimeline.id) return
      const pending = delivery.status === 'pending'
      setTimelineState(
        delivery.editorId === 'jianying'
          ? pending
            ? 'jianying-pending'
            : 'jianying'
          : 'exported',
      )
      setDeliveryNoticeTone('info')
      setDeliveryNotice(delivery.message)
      if (options.session?.conversationId) {
        await options.appendAgentMessage(
          options.session.conversationId,
          options.session.id,
          delivery.message,
        )
      }
    } catch (error) {
      setDeliveryNoticeTone('error')
      setDeliveryNotice(deliverErrorMessage(error, editorLabel))
    } finally {
      setIsDelivering(false)
    }
  }

  async function chooseOutputEditor(editorId: string) {
    if (!options.projectId) return
    try {
      const catalog = await setOutputEditor(options.projectId, editorId)
      setEditorCatalog(catalog)
    } catch (error) {
      const selected = editorCatalog.linkers.find((linker) => linker.id === editorId)
      setDeliveryNoticeTone('error')
      setDeliveryNotice(deliverErrorMessage(error, selected?.label ?? '编辑器'))
    }
  }

  function applyStudioCommit(nextTimeline: TimelineVersion, nextPreview: PreviewResult | null) {
    setTimeline(nextTimeline)
    activeTimelineRef.current = nextTimeline.id
    if (nextPreview) {
      applyPreview(nextPreview)
      setTimelineState('preview-ready')
    } else {
      setDeliveryNotice(null)
      setTimelineState('draft')
    }
  }

  // 返回后台任务 ID，供 App 层注册 pendingEdit 并驱动 reconciliation 轮询。
  return {
    storyboard,
    storyboardVersions,
    timeline,
    preview,
    previewNonce,
    timelineState,
    loadSession,
    applySessionSnapshot,
    applyAgentResult,
    applyStudioCommit,
    refreshAudit,
    reset,
    model: {
      shotImages: shotImages?.timelineId === timeline?.id ? shotImages?.images ?? {} : {},
      thumbnailNotice: shotImages?.timelineId === timeline?.id ? shotImages?.error : null,
      storyboard,
      storyboardVersions,
      timeline,
      preview,
      previewNonce,
      deliveryStatus: getDeliveryStatus(storyboard, timeline, preview, timelineState),
      deliveryNotice,
      deliveryNoticeTone,
      editorCatalog,
      selectedEditorId: editorCatalog.selectedId,
      deliverLabel: deliverActionLabel(editorCatalog.selectedId, isDelivering),
      busy: {
        creatingTimeline: isCreatingTimeline,
        renderingPreview: isRenderingPreview,
        delivering: isDelivering,
      },
    },
    actions: {
      openStoryboard: (storyboardVersionId: string) => void openStoryboard(storyboardVersionId),
      createTimeline: () => void createTimeline(),
      renderPreview: () => void createPreview(),
      deliverToEditor: (override?: TimelineVersion) => void deliverSelectedEditor(override),
      setOutputEditor: (editorId: string) => void chooseOutputEditor(editorId),
    },
  }
}

export type ArtifactWorkspaceController = ReturnType<typeof useArtifactWorkspaceController>
