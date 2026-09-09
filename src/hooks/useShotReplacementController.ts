// 手动换镜 controller：试选不落盘，保存创建新版本；旧预览保留到新预览完成。
import { useEffect, useRef, useState } from 'react'
import type { RefObject } from 'react'
import { commitStudioEdits, generateShotRecommendations, listShotRecommendations, prepareShotReplacement, renderPreview } from '../lib/local-store'
import type { PreparedShotReplacement, PreviewResult, ShotRecommendations, ShotScope, TimelineClipDto, TimelineVersion } from '../lib/local-store'

type Replacement = Pick<TimelineClipDto, 'shotIndex' | 'assetId' | 'sourceStartMs' | 'sourceEndMs' | 'cropFocus'>
type Change = { before: Replacement; after: Replacement }
type ContinueAction = (timeline?: TimelineVersion) => void
type Options = {
  projectId: string | null
  sessionId: string | null
  timeline: TimelineVersion | null
  activeProjectRef: RefObject<string | null>
  activeSessionRef: RefObject<string | null>
  applyCommit: (timeline: TimelineVersion, preview: PreviewResult | null) => void
}

export function useShotReplacementController(options: Options) {
  const [shot, setShot] = useState<TimelineClipDto | null>(null)
  const [recommendations, setRecommendations] = useState<ShotRecommendations | null>(null)
  const [selectedId, setSelectedId] = useState<string | null>(null)
  const [prepared, setPrepared] = useState<PreparedShotReplacement | null>(null)
  const [phase, setPhase] = useState<'idle' | 'loading' | 'preparing' | 'saving' | 'rendering'>('idle')
  const [notice, setNotice] = useState<string | null>(null)
  const [undo, setUndo] = useState<Change[]>([])
  const [redo, setRedo] = useState<Change[]>([])
  const [pending, setPending] = useState<ContinueAction | null>(null)
  const request = useRef(0)
  const expectedTimeline = useRef(options.timeline?.id)
  const timelineRef = useRef(options.timeline)
  const scopeKey = `${options.projectId}/${options.sessionId}`
  const previousScope = useRef(scopeKey)

  useEffect(() => {
    timelineRef.current = options.timeline
    if (expectedTimeline.current === options.timeline?.id && previousScope.current === scopeKey) return
    previousScope.current = scopeKey
    expectedTimeline.current = options.timeline?.id
    request.current += 1
    setShot(null)
    setPrepared(null)
    setSelectedId(null)
    setRecommendations(null)
    setPending(null)
    setPhase('idle')
    setNotice(null)
    setUndo([])
    setRedo([])
  }, [options.timeline, scopeKey])

  function close() {
    request.current += 1
    setShot(null)
    setSelectedId(null)
    setPrepared(null)
    setRecommendations(null)
    setPhase('idle')
    setPending(null)
  }

  function scope(clip: TimelineClipDto): ShotScope | null {
    if (!options.projectId || !options.sessionId || !timelineRef.current) return null
    return { projectId: options.projectId, editingTaskId: options.sessionId, timelineVersionId: timelineRef.current.id, shotIndex: clip.shotIndex }
  }

  function isCurrent(target: ShotScope, token: number) {
    return token === request.current && options.activeProjectRef.current === target.projectId
      && options.activeSessionRef.current === target.editingTaskId && timelineRef.current?.id === target.timelineVersionId
  }

  async function open(clip: TimelineClipDto, generate = false) {
    const target = scope(clip)
    if (!target) return
    const token = ++request.current
    setShot(timelineRef.current?.clips.find((item) => item.shotIndex === clip.shotIndex) ?? clip)
    setSelectedId(null)
    setPrepared(null)
    setNotice(null)
    setRecommendations(null)
    setPhase('loading')
    try {
      const result = await (generate ? generateShotRecommendations(target) : listShotRecommendations(target))
      if (isCurrent(target, token)) setRecommendations(result)
    } catch {
      if (isCurrent(target, token)) setNotice('无法读取此镜头的推荐，请确认素材可用；也可以通过对话调整。')
    } finally {
      if (isCurrent(target, token)) setPhase('idle')
    }
  }

  async function select(assetId: string) {
    if (!shot) return
    const target = scope(shot)
    if (!target) return
    const token = ++request.current
    setSelectedId(assetId)
    setPrepared(null)
    setNotice(null)
    setPhase('preparing')
    try {
      const result = await prepareShotReplacement(target, assetId)
      if (isCurrent(target, token)) setPrepared(result)
    } catch {
      if (isCurrent(target, token)) setNotice('此候选的画面预览未能准备完成，请检查模型配置和素材，或选择其他镜头。原剪辑未改变。')
    } finally {
      if (isCurrent(target, token)) setPhase('idle')
    }
  }

  async function commit(replacement: Replacement, onSaved: (next: TimelineVersion) => void) {
    if (!options.projectId || !options.sessionId || !options.timeline) return
    const target = { projectId: options.projectId, editingTaskId: options.sessionId, timelineVersionId: options.timeline.id, shotIndex: replacement.shotIndex }
    const token = ++request.current
    setPhase('saving')
    setNotice(null)
    let next: TimelineVersion
    try {
      const result = await commitStudioEdits({ ...target, clipReplacements: [replacement] })
      if (!isCurrent(target, token)) return
      next = result.timeline
      expectedTimeline.current = next.id
      timelineRef.current = next
      options.applyCommit(next, null)
      setShot(null)
      setPrepared(null)
      setSelectedId(null)
      onSaved(next)
    } catch {
      if (isCurrent(target, token)) {
        setNotice('修改未保存，原剪辑保持不变。请再次保存。')
        setPhase('idle')
      }
      return
    }
    const savedTarget = { ...target, timelineVersionId: next.id }
    if (!isCurrent(savedTarget, token)) return
    setPhase('rendering')
    setNotice('修改已保存，正在更新预览。当前仍显示上一版画面。')
    try {
      const preview = await renderPreview(next.id)
      if (!isCurrent(savedTarget, token)) return
      options.applyCommit(next, preview)
      setNotice('修改已保存，预览已更新。')
    } catch {
      if (isCurrent(savedTarget, token)) setNotice('修改已保存，但新预览生成失败。可重新生成预览；剪映草稿将使用已保存的镜头。')
    } finally {
      if (isCurrent(savedTarget, token)) setPhase('idle')
    }
  }

  async function save(action?: ContinueAction) {
    if (!shot || !prepared || phase !== 'idle') return
    const change = { before: shot, after: prepared }
    await commit(prepared, (next) => {
      setUndo((items) => [...items, change])
      setRedo([])
      setPending(null)
      action?.(next)
    })
  }

  function requestAction(action: ContinueAction) {
    if (phase === 'saving') return
    if (phase === 'rendering') { action(); return }
    if (selectedId) setPending(() => action)
    else { close(); action() }
  }

  return {
    model: { shot, recommendations, selectedId, prepared, phase, notice, pending: Boolean(pending), canUndo: undo.length > 0, canRedo: redo.length > 0 },
    actions: {
      open: (clip: TimelineClipDto) => requestAction(() => void open(clip)),
      generate: () => { if (shot) void open(shot, true) },
      select: (assetId: string) => void select(assetId),
      cancel: close,
      save: () => void save(),
      requestAction,
      keepEditing: () => setPending(null),
      saveAndContinue: () => { if (pending) void save(pending) },
      discardAndContinue: () => { const action = pending; close(); action?.() },
      undo: () => { const change = undo.at(-1); if (change) void commit(change.before, () => { setUndo((items) => items.slice(0, -1)); setRedo((items) => [...items, change]) }) },
      redo: () => { const change = redo.at(-1); if (change) void commit(change.after, () => { setRedo((items) => items.slice(0, -1)); setUndo((items) => [...items, change]) }) },
    },
  }
}

export type ShotReplacementController = ReturnType<typeof useShotReplacementController>
