// Studio 工作台 controller：以 TimelineVersion 为事实源的 mash 中间态。
// 对齐 OpenCut Classic 的 timeline 缩放 / 磁吸 / 波纹 / 播放头 模型。
// Stage 1-3C: 闭环剪辑 + 音频独立 + 叠加 + P2 多选/右键/磁吸扩展/音量

import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import type { PreviewResult, TimelineVersion } from '../lib/local-store'
import { mashToDiff, timelineToMash, type Mash } from '../lib/moviemasher-adapter'

export const STUDIO_BASE_PX_PER_SEC = 50
export const STUDIO_ZOOM_MIN = 0.2
export const STUDIO_ZOOM_MAX = 8
export const STUDIO_SNAP_THRESHOLD_PX = 8
const HISTORY_LIMIT = 50

export type StudioCommitState = 'idle' | 'committing' | 'success' | 'error'

export function useStudioWorkspaceController(
  timeline: TimelineVersion | null,
  preview: PreviewResult | null,
) {
  const [mash, setMash] = useState<Mash | null>(null)
  const [selectedClipIds, setSelectedClipIds] = useState<string[]>([])
  const selectedClipId = selectedClipIds[0] ?? null
  const setSelectedClipId = useCallback((id: string | null, opts?: { additive?: boolean }) => {
    if (opts?.additive && id) {
      setSelectedClipIds((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]))
    } else {
      setSelectedClipIds(id ? [id] : [])
    }
  }, [])
  const isSelected = useCallback((id: string) => selectedClipIds.includes(id), [selectedClipIds])
  const clearSelection = useCallback(() => setSelectedClipIds([]), [])
  const selectClipsInRange = useCallback((startMs: number, endMs: number, additive?: boolean) => {
    const cur = mashRef.current
    if (!cur) return
    const ids: string[] = []
    for (const track of cur.tracks) {
      for (const clip of track.clips) {
        if (clip.timelineEndMs > startMs && clip.timelineStartMs < endMs) ids.push(clip.id)
      }
    }
    if (additive) {
      setSelectedClipIds((prev) => {
        const set = new Set(prev)
        for (const id of ids) set.add(id)
        return [...set]
      })
    } else setSelectedClipIds(ids)
  }, [])

  const [commitState, setCommitState] = useState<StudioCommitState>('idle')
  const [commitError, setCommitError] = useState<string | null>(null)

  const [zoomLevel, setZoomLevel] = useState(1.2)
  const [snapEnabled, setSnapEnabled] = useState(true)
  const [rippleEnabled, setRippleEnabled] = useState(false)
  const [playheadMs, setPlayheadMs] = useState(0)

  const [trackUi, setTrackUi] = useState<Record<string, { muted: boolean; hidden: boolean; locked: boolean }>>({})

  const [undoStack, setUndoStack] = useState<Mash[]>([])
  const [redoStack, setRedoStack] = useState<Mash[]>([])

  const baseMash = useMemo(() => (timeline ? timelineToMash(timeline) : null), [timeline])
  const mashRef = useRef<Mash | null>(null)
  useEffect(() => { mashRef.current = mash }, [mash])

  useEffect(() => {
    if (baseMash) {
      setMash(baseMash)
      setSelectedClipIds([])
      setCommitState('idle')
      setCommitError(null)
      setPlayheadMs(0)
      setUndoStack([])
      setRedoStack([])
    } else {
      setMash(null)
    }
  }, [baseMash])

  const toggleTrackMute = useCallback((trackId: string) => {
    setTrackUi((prev) => {
      const cur = prev[trackId] ?? { muted: false, hidden: false, locked: false }
      return { ...prev, [trackId]: { ...cur, muted: !cur.muted } }
    })
  }, [])
  const toggleTrackHidden = useCallback((trackId: string) => {
    setTrackUi((prev) => {
      const cur = prev[trackId] ?? { muted: false, hidden: false, locked: false }
      return { ...prev, [trackId]: { ...cur, hidden: !cur.hidden } }
    })
  }, [])
  const toggleTrackLocked = useCallback((trackId: string) => {
    setTrackUi((prev) => {
      const cur = prev[trackId] ?? { muted: false, hidden: false, locked: false }
      return { ...prev, [trackId]: { ...cur, locked: !cur.locked } }
    })
  }, [])
  const isTrackLocked = useCallback((trackId: string) => !!trackUi[trackId]?.locked, [trackUi])
  const isTrackHidden = useCallback((trackId: string) => !!trackUi[trackId]?.hidden, [trackUi])
  const isTrackMuted = useCallback((trackId: string) => !!trackUi[trackId]?.muted, [trackUi])

  const diff = useMemo(() => {
    if (!timeline || !mash || !baseMash) return null
    if (mash.id !== timeline.id) return null
    return mashToDiff(timeline, mash)
  }, [timeline, mash, baseMash])

  const hasChanges = diff?.hasChanges ?? false

  const scale = (STUDIO_BASE_PX_PER_SEC * zoomLevel) / 1000
  const setScale = useCallback((nextScale: number) => {
    const nextZoom = Math.max(STUDIO_ZOOM_MIN, Math.min(STUDIO_ZOOM_MAX, (nextScale * 1000) / STUDIO_BASE_PX_PER_SEC))
    setZoomLevel(nextZoom)
  }, [])

  const pushHistory = useCallback(() => {
    const cur = mashRef.current
    if (!cur) return
    setUndoStack((prev) => [...prev, structuredClone(cur)].slice(-HISTORY_LIMIT))
    setRedoStack([])
  }, [])

  const applyMash = useCallback((updater: (prev: Mash) => Mash | null, withHistory = true) => {
    if (withHistory) {
      const cur = mashRef.current
      if (cur) {
        setUndoStack((prev) => [...prev, structuredClone(cur)].slice(-HISTORY_LIMIT))
        setRedoStack([])
      }
    }
    setMash((prev) => {
      if (!prev) return prev
      const next = updater(structuredClone(prev) as Mash)
      if (!next) return prev
      return next
    })
    setCommitState('idle')
  }, [])

  const applyMashTransient = useCallback((updater: (prev: Mash) => Mash | null) => {
    setMash((prev) => {
      if (!prev) return prev
      const next = updater(structuredClone(prev) as Mash)
      if (!next) return prev
      return next
    })
  }, [])

  const updateClipDuration = useCallback((clipId: string, newDurationMs: number, opts?: { ripple?: boolean }) => {
    const ripple = opts?.ripple ?? rippleEnabled
    applyMash((next) => {
      const videoTrack = next.tracks.find((t) => t.kind === 'video')
      if (!videoTrack) return null
      const clip = videoTrack.clips.find((c) => c.id === clipId)
      if (!clip) return null
      const clamped = Math.max(200, Math.min(12000, Math.round(newDurationMs)))
      const oldDur = clip.timelineEndMs - clip.timelineStartMs
      const delta = clamped - oldDur
      if (delta === 0) return null
      clip.timelineEndMs = clip.timelineStartMs + clamped
      clip.sourceEndMs = clip.sourceStartMs + clamped
      if (!ripple) {
        const idx = videoTrack.clips.findIndex((c) => c.id === clipId)
        for (let i = idx + 1; i < videoTrack.clips.length; i += 1) {
          videoTrack.clips[i].timelineStartMs += delta
          videoTrack.clips[i].timelineEndMs += delta
        }
        next.durationMs += delta
      } else {
        next.durationMs = Math.max(...videoTrack.clips.map((c) => c.timelineEndMs), 0)
      }
      return next
    })
  }, [applyMash, rippleEnabled])

  const updateClipDurationTransient = useCallback((clipId: string, newDurationMs: number, opts?: { ripple?: boolean }) => {
    const ripple = opts?.ripple ?? rippleEnabled
    applyMashTransient((next) => {
      const videoTrack = next.tracks.find((t) => t.kind === 'video')
      if (!videoTrack) return null
      const clip = videoTrack.clips.find((c) => c.id === clipId)
      if (!clip) return null
      const clamped = Math.max(200, Math.min(12000, Math.round(newDurationMs)))
      const oldDur = clip.timelineEndMs - clip.timelineStartMs
      const delta = clamped - oldDur
      if (delta === 0) return null
      clip.timelineEndMs = clip.timelineStartMs + clamped
      clip.sourceEndMs = clip.sourceStartMs + clamped
      if (!ripple) {
        const idx = videoTrack.clips.findIndex((c) => c.id === clipId)
        for (let i = idx + 1; i < videoTrack.clips.length; i += 1) {
          videoTrack.clips[i].timelineStartMs += delta
          videoTrack.clips[i].timelineEndMs += delta
        }
        next.durationMs += delta
      } else {
        next.durationMs = Math.max(...videoTrack.clips.map((c) => c.timelineEndMs), 0)
      }
      return next
    })
  }, [applyMashTransient, rippleEnabled])

  const resizeClipLeft = useCallback((clipId: string, deltaMs: number) => {
    applyMash((next) => {
      const track = next.tracks.find((t) => t.kind === 'video')
      if (!track) return null
      const idx = track.clips.findIndex((c) => c.id === clipId)
      if (idx === -1) return null
      const clip = track.clips[idx]
      const prev = idx > 0 ? track.clips[idx - 1] : null
      const prevEnd = prev ? prev.timelineEndMs : 0
      const dur = clip.timelineEndMs - clip.timelineStartMs
      const maxShrink = dur - 200
      const maxExpandLeft = clip.timelineStartMs - prevEnd
      const maxExpandSource = clip.sourceStartMs
      let clampedDelta = deltaMs
      if (clampedDelta > 0) clampedDelta = Math.min(clampedDelta, maxShrink)
      else clampedDelta = Math.max(clampedDelta, -Math.min(maxExpandLeft, maxExpandSource))
      if (clampedDelta === 0) return null
      clip.timelineStartMs += clampedDelta
      clip.sourceStartMs += clampedDelta
      return next
    })
  }, [applyMash])

  const resizeClipLeftTransient = useCallback((clipId: string, deltaMs: number) => {
    applyMashTransient((next) => {
      const track = next.tracks.find((t) => t.kind === 'video')
      if (!track) return null
      const idx = track.clips.findIndex((c) => c.id === clipId)
      if (idx === -1) return null
      const clip = track.clips[idx]
      const prev = idx > 0 ? track.clips[idx - 1] : null
      const prevEnd = prev ? prev.timelineEndMs : 0
      const dur = clip.timelineEndMs - clip.timelineStartMs
      const maxShrink = dur - 200
      const maxExpandLeft = clip.timelineStartMs - prevEnd
      const maxExpandSource = clip.sourceStartMs
      let clampedDelta = deltaMs
      if (clampedDelta > 0) clampedDelta = Math.min(clampedDelta, maxShrink)
      else clampedDelta = Math.max(clampedDelta, -Math.min(maxExpandLeft, maxExpandSource))
      if (clampedDelta === 0) return null
      clip.timelineStartMs += clampedDelta
      clip.sourceStartMs += clampedDelta
      return next
    })
  }, [applyMashTransient])

  const reorderClips = useCallback((fromIndex: number, toIndex: number) => {
    applyMash((next) => {
      const track = next.tracks.find((t) => t.kind === 'video')
      if (!track) return null
      if (fromIndex < 0 || fromIndex >= track.clips.length) return null
      if (toIndex < 0 || toIndex >= track.clips.length) return null
      if (fromIndex === toIndex) return null
      const [moved] = track.clips.splice(fromIndex, 1)
      track.clips.splice(toIndex, 0, moved)
      let cursor = 0
      for (const clip of track.clips) {
        const dur = clip.timelineEndMs - clip.timelineStartMs
        clip.timelineStartMs = cursor
        clip.timelineEndMs = cursor + dur
        cursor += dur
      }
      next.durationMs = cursor
      return next
    })
  }, [applyMash])

  const reorderClipsWithoutHistory = useCallback((fromIndex: number, toIndex: number) => {
    applyMashTransient((next) => {
      const track = next.tracks.find((t) => t.kind === 'video')
      if (!track) return null
      if (fromIndex < 0 || fromIndex >= track.clips.length) return null
      if (toIndex < 0 || toIndex >= track.clips.length) return null
      if (fromIndex === toIndex) return null
      const [moved] = track.clips.splice(fromIndex, 1)
      track.clips.splice(toIndex, 0, moved)
      let cursor = 0
      for (const clip of track.clips) {
        const dur = clip.timelineEndMs - clip.timelineStartMs
        clip.timelineStartMs = cursor
        clip.timelineEndMs = cursor + dur
        cursor += dur
      }
      next.durationMs = cursor
      return next
    })
  }, [applyMashTransient])

  const splitClip = useCallback((clipId: string, atMs: number) => {
    applyMash((next) => {
      const track = next.tracks.find((t) => t.kind === 'video')
      if (!track) return null
      const idx = track.clips.findIndex((c) => c.id === clipId)
      if (idx === -1) return null
      const clip = track.clips[idx]
      if (atMs <= clip.timelineStartMs + 120 || atMs >= clip.timelineEndMs - 120) return null
      const leftDur = atMs - clip.timelineStartMs
      const rightDur = clip.timelineEndMs - atMs
      if (leftDur < 200 || rightDur < 200) return null
      const leftSourceEnd = clip.sourceStartMs + leftDur
      const rightSourceStart = leftSourceEnd
      const rightClip = {
        ...structuredClone(clip),
        id: `${clip.id}_split_${Date.now().toString(36)}`,
        timelineStartMs: atMs,
        timelineEndMs: clip.timelineEndMs,
        sourceStartMs: rightSourceStart,
        sourceEndMs: clip.sourceEndMs,
      }
      clip.timelineEndMs = atMs
      clip.sourceEndMs = leftSourceEnd
      track.clips.splice(idx + 1, 0, rightClip)
      next.durationMs = Math.max(...track.clips.map((c) => c.timelineEndMs), next.durationMs)
      return next
    })
  }, [applyMash])

  const splitClipAtPlayhead = useCallback(() => {
    const cur = mashRef.current
    if (!cur) return false
    const track = cur.tracks.find((t) => t.kind === 'video')
    if (!track) return false
    let target: typeof track.clips[number] | null = null
    if (selectedClipId) {
      const sel = track.clips.find((c) => c.id === selectedClipId)
      if (sel && playheadMs > sel.timelineStartMs && playheadMs < sel.timelineEndMs) target = sel
    }
    if (!target) {
      target = track.clips.find((c) => playheadMs > c.timelineStartMs && playheadMs < c.timelineEndMs) ?? null
    }
    if (!target) return false
    splitClip(target.id, playheadMs)
    return true
  }, [playheadMs, selectedClipId, splitClip])

  const deleteClip = useCallback((clipId: string) => {
    applyMash((next) => {
      let deleted = false
      for (const track of next.tracks) {
        const idx = track.clips.findIndex((c) => c.id === clipId)
        if (idx !== -1) {
          track.clips.splice(idx, 1)
          deleted = true
          if (track.kind === 'video') {
            let cursor = 0
            for (const clip of track.clips) {
              const dur = clip.timelineEndMs - clip.timelineStartMs
              clip.timelineStartMs = cursor
              clip.timelineEndMs = cursor + dur
              cursor += dur
            }
            next.durationMs = cursor
          }
          if (track.kind === 'overlay' || track.kind === 'audio') {
            next.durationMs = Math.max(0, ...next.tracks.flatMap((t) => t.clips.map((c) => c.timelineEndMs)))
          }
          break
        }
      }
      if (!deleted) return null
      return next
    })
    setSelectedClipIds((prev) => prev.filter((id) => id !== clipId))
  }, [applyMash])

  const deleteSelected = useCallback(() => {
    if (selectedClipIds.length === 0) return false
    const cur = mashRef.current
    if (!cur) return false
    const videoTrack = cur.tracks.find((t) => t.kind === 'video')
    const videoSelectedCount = selectedClipIds.filter((id) => videoTrack?.clips.some((c) => c.id === id)).length
    if (videoTrack && videoTrack.clips.length - videoSelectedCount < 1) return false
    const ids = [...selectedClipIds]
    applyMash((next) => {
      let changed = false
      for (const id of ids) {
        for (const track of next.tracks) {
          const idx = track.clips.findIndex((c) => c.id === id)
          if (idx !== -1) {
            track.clips.splice(idx, 1)
            changed = true
            break
          }
        }
      }
      if (!changed) return null
      const vt = next.tracks.find((t) => t.kind === 'video')
      if (vt) {
        let cursor = 0
        for (const clip of vt.clips) {
          const dur = clip.timelineEndMs - clip.timelineStartMs
          clip.timelineStartMs = cursor
          clip.timelineEndMs = cursor + dur
          cursor += dur
        }
        next.durationMs = Math.max(cursor, ...next.tracks.flatMap((t) => t.clips.map((c) => c.timelineEndMs)))
      } else {
        next.durationMs = Math.max(0, ...next.tracks.flatMap((t) => t.clips.map((c) => c.timelineEndMs)))
      }
      return next
    })
    setSelectedClipIds([])
    return true
  }, [selectedClipIds, applyMash])

  const addOverlayAtPlayhead = useCallback(() => {
    const cur = mashRef.current
    if (!cur) return false
    const videoTrack = cur.tracks.find((t) => t.kind === 'video')
    const selected = selectedClipId ? cur.tracks.flatMap((t) => t.clips).find((c) => c.id === selectedClipId) : null
    const sourceClip = selected && selected.assetId ? selected : videoTrack?.clips[0]
    if (!sourceClip || !sourceClip.assetId) return false
    const dur = Math.min(2500, sourceClip.timelineEndMs - sourceClip.timelineStartMs)
    const startMs = Math.max(0, Math.min(playheadMs, (cur.durationMs || 0)))
    applyMash((next) => {
      let overlayTrack = next.tracks.find((t) => t.kind === 'overlay')
      if (!overlayTrack) {
        overlayTrack = { id: 'overlay-main', kind: 'overlay', label: '叠加', enabled: true, clips: [] }
        const firstTextIdx = next.tracks.findIndex((t) => t.kind === 'text')
        if (firstTextIdx === -1) next.tracks.push(overlayTrack)
        else next.tracks.splice(firstTextIdx, 0, overlayTrack)
      }
      const nextShotIndex = Math.max(0, ...next.tracks.flatMap((t) => t.kind === 'overlay' ? t.clips.map((c) => Number.parseInt(c.id.replace('overlay-', ''), 10) || 0) : []), 9999) + 1
      const clip = {
        id: `overlay-new-${Date.now().toString(36)}-${nextShotIndex}`,
        kind: 'overlay' as const,
        assetId: sourceClip.assetId,
        text: sourceClip.text,
        timelineStartMs: startMs,
        timelineEndMs: startMs + dur,
        sourceStartMs: sourceClip.sourceStartMs,
        sourceEndMs: sourceClip.sourceStartMs + dur,
        trackId: overlayTrack.id,
        role: null,
      }
      overlayTrack.clips.push(clip)
      overlayTrack.clips.sort((a, b) => a.timelineStartMs - b.timelineStartMs)
      next.durationMs = Math.max(next.durationMs, ...next.tracks.flatMap((t) => t.clips.map((c) => c.timelineEndMs)))
      return next
    })
    return true
  }, [applyMash, playheadMs, selectedClipId])

  const moveOverlayClipTransient = useCallback((clipId: string, absoluteStartMs: number) => {
    applyMashTransient((next) => {
      const track = next.tracks.find((t) => t.kind === 'overlay')
      if (!track) return null
      const clip = track.clips.find((c) => c.id === clipId)
      if (!clip) return null
      const dur = clip.timelineEndMs - clip.timelineStartMs
      const nextStart = Math.max(0, Math.round(absoluteStartMs))
      clip.timelineStartMs = nextStart
      clip.timelineEndMs = nextStart + dur
      const maxEnd = Math.max(next.durationMs, ...next.tracks.flatMap((t) => t.clips.map((c) => c.timelineEndMs)))
      next.durationMs = maxEnd
      track.clips.sort((a, b) => a.timelineStartMs - b.timelineStartMs)
      return next
    })
  }, [applyMashTransient])

  const resizeOverlayLeftTransient = useCallback((clipId: string, nextStartMs: number) => {
    applyMashTransient((next) => {
      const track = next.tracks.find((t) => t.kind === 'overlay')
      if (!track) return null
      const clip = track.clips.find((c) => c.id === clipId)
      if (!clip) return null
      const endMs = clip.timelineEndMs
      const ns = Math.max(0, Math.round(nextStartMs))
      const dur = endMs - ns
      if (dur < 200 || dur > 12000) return null
      clip.timelineStartMs = ns
      clip.timelineEndMs = endMs
      clip.sourceEndMs = clip.sourceStartMs + dur
      track.clips.sort((a, b) => a.timelineStartMs - b.timelineStartMs)
      return next
    })
  }, [applyMashTransient])

  const updateOverlayClipDurationTransient = useCallback((clipId: string, newDurationMs: number) => {
    applyMashTransient((next) => {
      const track = next.tracks.find((t) => t.kind === 'overlay')
      if (!track) return null
      const clip = track.clips.find((c) => c.id === clipId)
      if (!clip) return null
      const clamped = Math.max(200, Math.min(12000, Math.round(newDurationMs)))
      const dur = clamped
      clip.timelineEndMs = clip.timelineStartMs + dur
      clip.sourceEndMs = clip.sourceStartMs + dur
      next.durationMs = Math.max(...next.tracks.flatMap((t) => t.clips.map((c) => c.timelineEndMs)), next.durationMs)
      return next
    })
  }, [applyMashTransient])

  const updateOverlayClipDuration = useCallback((clipId: string, newDurationMs: number) => {
    applyMash((next) => {
      const track = next.tracks.find((t) => t.kind === 'overlay')
      if (!track) return null
      const clip = track.clips.find((c) => c.id === clipId)
      if (!clip) return null
      const clamped = Math.max(200, Math.min(12000, Math.round(newDurationMs)))
      clip.timelineEndMs = clip.timelineStartMs + clamped
      clip.sourceEndMs = clip.sourceStartMs + clamped
      next.durationMs = Math.max(...next.tracks.flatMap((t) => t.clips.map((c) => c.timelineEndMs)), next.durationMs)
      return next
    })
  }, [applyMash])

  const updateAudioVolume = useCallback((cueId: string, newVolume: number) => {
    const vol = Math.max(0, Math.min(2, newVolume))
    applyMash((next) => {
      for (const track of next.tracks) {
        if (track.kind !== 'audio') continue
        const clip = track.clips.find((c) => c.id === cueId)
        if (clip) {
          clip.volume = vol
          return next
        }
      }
      return null
    })
  }, [applyMash])

  const updateAudioVolumeTransient = useCallback((cueId: string, newVolume: number) => {
    const vol = Math.max(0, Math.min(2, newVolume))
    applyMashTransient((next) => {
      for (const track of next.tracks) {
        if (track.kind !== 'audio') continue
        const clip = track.clips.find((c) => c.id === cueId)
        if (clip) {
          clip.volume = vol
          return next
        }
      }
      return null
    })
  }, [applyMashTransient])

  const duplicateClip = useCallback((clipId: string) => {
    const cur = mashRef.current
    const owningTrack = cur?.tracks.find((t) => t.clips.some((c) => c.id === clipId))
    if (owningTrack?.kind === 'overlay') {
      applyMash((next) => {
        const track = next.tracks.find((t) => t.kind === 'overlay')
        if (!track) return null
        const idx = track.clips.findIndex((c) => c.id === clipId)
        if (idx === -1) return null
        const clip = track.clips[idx]
        const dur = clip.timelineEndMs - clip.timelineStartMs
        const clone = {
          ...structuredClone(clip),
          id: `overlay-new-${Date.now().toString(36)}`,
          timelineStartMs: clip.timelineStartMs + 400,
          timelineEndMs: clip.timelineStartMs + 400 + dur,
        }
        track.clips.push(clone)
        track.clips.sort((a, b) => a.timelineStartMs - b.timelineStartMs)
        next.durationMs = Math.max(next.durationMs, clone.timelineEndMs)
        return next
      })
      return
    }
    if (owningTrack?.kind === 'audio') {
      // audio duplicate not supported in preview splice; just return
      return
    }
    applyMash((next) => {
      const track = next.tracks.find((t) => t.kind === 'video')
      if (!track) return null
      const idx = track.clips.findIndex((c) => c.id === clipId)
      if (idx === -1) return null
      const clip = track.clips[idx]
      const dur = clip.timelineEndMs - clip.timelineStartMs
      const clone = {
        ...structuredClone(clip),
        id: `${clip.id}_copy_${Date.now().toString(36)}`,
        timelineStartMs: clip.timelineEndMs,
        timelineEndMs: clip.timelineEndMs + dur,
      }
      track.clips.splice(idx + 1, 0, clone)
      let cursor = 0
      for (const c of track.clips) {
        const d = c.timelineEndMs - c.timelineStartMs
        c.timelineStartMs = cursor
        c.timelineEndMs = cursor + d
        cursor += d
      }
      next.durationMs = cursor
      return next
    })
  }, [applyMash])

  const duplicateSelected = useCallback(() => {
    if (selectedClipIds.length === 0) return false
    // duplicate each selected video/overlay clip
    const ids = [...selectedClipIds]
    for (const id of ids) duplicateClip(id)
    return true
  }, [selectedClipIds, duplicateClip])

  const undo = useCallback(() => {
    setUndoStack((prev) => {
      if (prev.length === 0) return prev
      const last = prev[prev.length - 1]
      const cur = mashRef.current
      if (cur) setRedoStack((r) => [...r, structuredClone(cur)].slice(-HISTORY_LIMIT))
      setMash(structuredClone(last))
      setCommitState('idle')
      return prev.slice(0, -1)
    })
  }, [])

  const redo = useCallback(() => {
    setRedoStack((prev) => {
      if (prev.length === 0) return prev
      const last = prev[prev.length - 1]
      const cur = mashRef.current
      if (cur) setUndoStack((u) => [...u, structuredClone(cur)].slice(-HISTORY_LIMIT))
      setMash(structuredClone(last))
      setCommitState('idle')
      return prev.slice(0, -1)
    })
  }, [])

  const canUndo = undoStack.length > 0
  const canRedo = redoStack.length > 0

  const updateTextCue = useCallback((cueId: string, text: string) => {
    applyMash((next) => {
      let found = false
      for (const track of next.tracks) {
        if (track.kind !== 'text') continue
        const clip = track.clips.find((c) => c.id === cueId)
        if (clip) {
          clip.text = text.slice(0, 280)
          found = true
          break
        }
      }
      return found ? next : null
    })
  }, [applyMash])

  const updateTextTemplate = useCallback((cueId: string, templateId: string | null) => {
    applyMash((next) => {
      let found = false
      for (const track of next.tracks) {
        if (track.kind !== 'text') continue
        const clip = track.clips.find((c) => c.id === cueId)
        if (clip) {
          clip.templateId = templateId
          found = true; break
        }
      }
      return found ? next : null
    })
  }, [applyMash])

  const updateTextStyle = useCallback((cueId: string, patch: Partial<NonNullable<import('../lib/moviemasher-adapter').MashClip['textStyle']>>) => {
    applyMash((next) => {
      let found = false
      for (const track of next.tracks) {
        if (track.kind !== 'text') continue
        const clip = track.clips.find((c) => c.id === cueId)
        if (clip) {
          clip.textStyle = { ...(clip.textStyle ?? { fontKey: 'jianying_default', fontSize: 0.055, bold: true, color: '#FFFFFF', strokeColor: null, strokeWidth: 0, shadow: false, backgroundColor: null, alignment: 'center', letterSpacing: 0, lineSpacing: 0 }), ...patch } as NonNullable<typeof clip.textStyle>
          found = true; break
        }
      }
      return found ? next : null
    })
  }, [applyMash])

  const updateTextLayout = useCallback((cueId: string, patch: Partial<NonNullable<import('../lib/moviemasher-adapter').MashClip['textLayout']>>) => {
    applyMash((next) => {
      let found = false
      for (const track of next.tracks) {
        if (track.kind !== 'text') continue
        const clip = track.clips.find((c) => c.id === cueId)
        if (clip) {
          clip.textLayout = { ...(clip.textLayout ?? { anchor: 'bottom', x: 0.5, y: 0.82, maxWidth: 0.86, safeArea: 'title_safe' }), ...patch } as NonNullable<typeof clip.textLayout>
          found = true; break
        }
      }
      return found ? next : null
    })
  }, [applyMash])

  const updateTextAnimation = useCallback((cueId: string, kind: 'entrance' | 'exit' | 'loop', animation: import('../lib/moviemasher-adapter').MashClip['textEntrance']) => {
    applyMash((next) => {
      for (const track of next.tracks) {
        if (track.kind !== 'text') continue
        const clip = track.clips.find((c) => c.id === cueId)
        if (clip) {
          if (kind === 'entrance') clip.textEntrance = animation ?? null
          else if (kind === 'exit') clip.textExit = animation ?? null
          else clip.textLoop = animation ?? null
          return next
        }
      }
      return null
    })
  }, [applyMash])

  const updateTextTrackMeta = useCallback((trackId: string, patch: Partial<{ enabled: boolean; locked: boolean; editable: boolean }>) => {
    applyMash((next) => {
      const track = next.tracks.find((t) => t.id === trackId && t.kind === 'text')
      if (!track) return null
      if (typeof patch.enabled === 'boolean') track.enabled = patch.enabled
      if (typeof patch.locked === 'boolean') track.locked = patch.locked
      if (typeof patch.editable === 'boolean') track.editable = patch.editable
      return next
    })
  }, [applyMash])

  const replaceVideoAsset = useCallback((clipId: string, newAssetId: string) => {
    applyMash((next) => {
      for (const track of next.tracks) {
        if (track.kind !== 'video' && track.kind !== 'overlay') continue
        const clip = track.clips.find((c) => c.id === clipId)
        if (clip) {
          const dur = clip.timelineEndMs - clip.timelineStartMs
          clip.assetId = newAssetId
          clip.sourceStartMs = 0
          clip.sourceEndMs = dur
          return next
        }
      }
      return null
    })
  }, [applyMash])

  const updateClipSourceWindow = useCallback((clipId: string, newSourceStartMs: number) => {
    const s = Math.max(0, Math.round(newSourceStartMs))
    applyMash((next) => {
      for (const track of next.tracks) {
        if (track.kind !== 'video' && track.kind !== 'overlay') continue
        const clip = track.clips.find((c) => c.id === clipId)
        if (clip) {
          const dur = clip.timelineEndMs - clip.timelineStartMs
          clip.sourceStartMs = s
          clip.sourceEndMs = s + dur
          return next
        }
      }
      return null
    })
  }, [applyMash])

  const updateAudioFadeLoop = useCallback((cueId: string, patch: Partial<{ fadeInMs: number; fadeOutMs: number; loopEnabled: boolean }>) => {
    applyMash((next) => {
      for (const track of next.tracks) {
        if (track.kind !== 'audio') continue
        const clip = track.clips.find((c) => c.id === cueId)
        if (clip) {
          if (typeof patch.fadeInMs === 'number') clip.fadeInMs = Math.max(0, Math.round(patch.fadeInMs))
          if (typeof patch.fadeOutMs === 'number') clip.fadeOutMs = Math.max(0, Math.round(patch.fadeOutMs))
          if (typeof patch.loopEnabled === 'boolean') clip.loopEnabled = patch.loopEnabled
          return next
        }
      }
      return null
    })
  }, [applyMash])

  const toggleAudioTrackEnabled = useCallback((trackId: string) => {
    applyMash((next) => {
      const track = next.tracks.find((t) => t.id === trackId && t.kind === 'audio')
      if (!track) return null
      track.enabled = !track.enabled
      return next
    })
  }, [applyMash])

  const reset = useCallback(() => {
    if (baseMash) {
      const cur = mashRef.current
      if (cur) setUndoStack((prev) => [...prev, structuredClone(cur)].slice(-HISTORY_LIMIT))
      setMash(baseMash)
      setCommitState('idle')
      setCommitError(null)
    }
  }, [baseMash])

  return {
    mash,
    baseMash,
    diff,
    hasChanges,
    selectedClipId,
    selectedClipIds,
    setSelectedClipId,
    setSelectedClipIds,
    isSelected,
    clearSelection,
    selectClipsInRange,
    scale,
    setScale,
    zoomLevel,
    setZoomLevel,
    snapEnabled,
    setSnapEnabled,
    rippleEnabled,
    setRippleEnabled,
    playheadMs,
    setPlayheadMs,
    trackUi,
    toggleTrackMute,
    toggleTrackHidden,
    toggleTrackLocked,
    isTrackLocked,
    isTrackHidden,
    isTrackMuted,
    preview,
    timeline,
    commitState,
    commitError,
    setCommitState,
    setCommitError,
    updateClipDuration,
    updateClipDurationTransient,
    resizeClipLeft,
    resizeClipLeftTransient,
    addOverlayAtPlayhead,
    moveOverlayClipTransient,
    resizeOverlayLeftTransient,
    updateOverlayClipDuration,
    updateOverlayClipDurationTransient,
    updateAudioVolume,
    updateAudioVolumeTransient,
    pushHistory,
    reorderClips,
    splitClip,
    splitClipAtPlayhead,
    deleteClip,
    deleteSelected,
    duplicateClip,
    duplicateSelected,
    undo,
    redo,
    canUndo,
    canRedo,
    reorderClipsWithoutHistory,
    updateTextCue,
    updateTextTemplate,
    updateTextStyle,
    updateTextLayout,
    updateTextAnimation,
    updateTextTrackMeta,
    replaceVideoAsset,
    updateClipSourceWindow,
    updateAudioFadeLoop,
    toggleAudioTrackEnabled,
    reset,
  }
}

export type StudioWorkspaceController = ReturnType<typeof useStudioWorkspaceController>
