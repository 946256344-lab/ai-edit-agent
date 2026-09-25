// 产物工作区 controller：加载 storyboard/timeline/preview，并按所选输出端口交付。
import { useEffect, useRef, useState } from 'react'
import type { Dispatch, RefObject, SetStateAction } from 'react'
import { listen } from '@tauri-apps/api/event'
import { messages } from '../lib/i18n'
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
  const status = messages().output.status
  if (!storyboard) return status.waiting
  if (!timeline) return status.shotsSelected
  if (timelineState === 'preview-generating') return status.previewGenerating
  if (timelineState === 'jianying-pending') return status.jianyingPending
  if (timelineState === 'jianying') return status.jianyingReady
  if (timelineState === 'exported') return status.exported
  if (!preview) return status.awaitingPreview
  return status.previewReady
}

function deliverErrorMessage(error: unknown, editorLabel: string) {
  const raw = error instanceof Error ? error.message : String(error ?? '')
  const copy = messages().output.errors
  if (/尚未实现/.test(raw)) return raw // i18n-allow: 匹配 Rust 返回的中文原文
  if (/draft library is unavailable/i.test(raw)) {
    return copy.noDraftLibrary
  }
  if (/not yet verified for Jianying/i.test(raw)) {
    return copy.subtitleUnsupported
  }
  if (/Python with a draft SDK.*unavailable|No module named 'pyJianYingDraft'|No module named 'pycapcut'/i.test(raw)) {
    return copy.missingSdk
  }
  if (/source media|unavailable asset|Music source|Voiceover media/i.test(raw)) {
    return copy.missingMedia(editorLabel)
  }
  if (/Jianying Pro is still running/i.test(raw)) {
    return copy.jianyingRunning
  }
  if (/无法写出|无法准备/.test(raw)) { // i18n-allow: 匹配 Rust 返回的中文原文
    return copy.writeFailed(editorLabel)
  }
  // 适配器报告的具体原因：优先取 Python 侧最内层的原因，方便排查。
  const adapterReason =
    raw.match(/adapter failed:\s*(.+)$/is)?.[1]?.trim() ??
    raw.match(/adapter could not create a draft:\s*(.+)$/is)?.[1]?.trim()
  if (adapterReason) {
    return copy.adapterFailed(editorLabel, adapterReason)
  }
  return copy.generic(editorLabel)
}

export function deliverActionLabel(editorId: string, busy: boolean) {
  const copy = messages().output
  if (busy) return copy.delivering
  if (editorId === 'fcpxml') return copy.exportFcpxml
  if (editorId === 'otio') return copy.exportOtio
  if (editorId === 'capcut') return copy.createCapcut
  return copy.createJianying
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
      .catch(() => { if (active) setShotImages({ timelineId: timeline.id, images: {}, error: messages().output.thumbnailsFailed }) })
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
          messages().output.timelineCreatedMessage,
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
          messages().output.previewReadyMessage,
        )
      }
    } catch {
      if (activeTimelineRef.current === timeline.id && options.activeSessionRef.current === options.sessionId) {
        setTimelineState('draft')
        setDeliveryNotice(messages().output.previewFailed)
      }
    } finally {
      setIsRenderingPreview(false)
    }
  }

  async function deliverSelectedEditor(override?: TimelineVersion) {
    const deliveryTimeline = override ?? timeline
    const selected = editorCatalog.linkers.find((linker) => linker.id === editorCatalog.selectedId)
    const editorLabel = selected?.label ?? messages().output.editorFallback
    if (!options.projectId || !deliveryTimeline || isDelivering) {
      if (!timeline) {
        setDeliveryNoticeTone('error')
        setDeliveryNotice(messages().output.nothingToDeliver)
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
      setDeliveryNotice(deliverErrorMessage(error, selected?.label ?? messages().output.editorFallback))
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
