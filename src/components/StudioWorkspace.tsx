// Studio v2 Comprehensive — 事实源仍为 Rust TimelineVersion，Inspector 全字段露出
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { convertFileSrc } from '@tauri-apps/api/core'
import type { PreviewResult, TimelineVersion, StoredAsset } from '../lib/local-store'
import { commitStudioEdits, getAssetEvidence, listAssetPage, renderPreview } from '../lib/local-store'
import { formatMs } from '../lib/moviemasher-adapter'
import type { StudioWorkspaceController } from '../hooks/useStudioWorkspaceController'
import { STUDIO_BASE_PX_PER_SEC, STUDIO_ZOOM_MAX, STUDIO_ZOOM_MIN } from '../hooks/useStudioWorkspaceController'
import type { Mash } from '../lib/moviemasher-adapter'
import { SUBTITLE_PRESETS } from '../lib/subtitle-presets'
import { attachJassub } from '../lib/jassub-adapter'
import { FPS_30 } from '../opencut/time/frameRate'
import { formatRulerLabel, getRulerConfig as getOpencutRulerConfig } from '../opencut/timeline/ruler-utils'

type StudioWorkspaceProps = {
  controller: StudioWorkspaceController
  projectId: string | null
  sessionId: string | null
  timeline: TimelineVersion | null
  preview: PreviewResult | null
  previewNonce: number
  onCommitted: (timeline: TimelineVersion, preview: PreviewResult | null) => void
}

const RULER_H = 22
const GAP = 6
const PADDING_TOP = 2
const TRACK_H: Record<string, number> = { video: 65, overlay: 56, text: 26, audio: 50, graphic: 26, effect: 26 }

function sliderToZoom(slider: number, minZoom: number) {
  const minLog = Math.log(minZoom), maxLog = Math.log(STUDIO_ZOOM_MAX)
  return Math.exp(minLog + slider * (maxLog - minLog))
}
function zoomToSlider(zoom: number, minZoom: number) {
  const minLog = Math.log(minZoom), maxLog = Math.log(STUDIO_ZOOM_MAX)
  return (Math.log(zoom) - minLog) / (maxLog - minLog)
}
function fmtRulerLabel(s: number): string {
  return formatRulerLabel({ timeInSeconds: s, fps: FPS_30 })
}

export function StudioWorkspace({ controller, projectId, sessionId, timeline, preview, previewNonce, onCommitted }: StudioWorkspaceProps) {
  const { mash, diff, hasChanges, selectedClipId, selectedClipIds, zoomLevel, setZoomLevel, snapEnabled, setSnapEnabled, rippleEnabled, setRippleEnabled, playheadMs, setPlayheadMs, canUndo, canRedo } = controller
  const rulerScrollRef = useRef<HTMLDivElement>(null)
  const tracksScrollRef = useRef<HTMLDivElement>(null)
  const trackLabelsScrollRef = useRef<HTMLDivElement>(null)
  const timelineRef = useRef<HTMLDivElement>(null)
  const videoRef = useRef<HTMLVideoElement>(null)
  const jassubContainerRef = useRef<HTMLDivElement>(null)
  const jassubHandleRef = useRef<{ destroy: () => void } | null>(null)
  const [drag, setDrag] = useState<null | { type: 'move' | 'resize-left' | 'resize-right', clipId: string, startX: number, startMs: number, startDur: number }>(null)
  const [scrubbing, setScrubbing] = useState(false)
  const [snapX, setSnapX] = useState<number | null>(null)
  const [thumbMap, setThumbMap] = useState<Record<string, string>>({})
  const prevAssetKeyRef = useRef<string>('')
  const [renderingPreview, setRenderingPreview] = useState(false)
  const [contextMenu, setContextMenu] = useState<null | { x: number; y: number; clipId: string | null }>(null)
  const [marquee, setMarquee] = useState<null | { startX: number; curX: number; startY: number; curY: number }>(null)
  const [pickerOpen, setPickerOpen] = useState(false)
  const [pickerTarget, setPickerTarget] = useState<string | null>(null)
  const [pickerAssets, setPickerAssets] = useState<StoredAsset[]>([])
  const [pickerQuery, setPickerQuery] = useState('')
  const pps = STUDIO_BASE_PX_PER_SEC * zoomLevel
  const scale = pps / 1000
  const durationMs = mash?.durationMs ?? 0
  const contentW = Math.max(640, durationMs * scale + 200)
  const minZoom = useMemo(() => {
    const containerW = 800
    return Math.max(STUDIO_ZOOM_MIN, Math.min(1, containerW / Math.max(1, durationMs / 1000 * STUDIO_BASE_PX_PER_SEC)))
  }, [durationMs])
  const syncFollowers = useCallback(() => {
    const tracks = tracksScrollRef.current
    if (!tracks) return
    if (rulerScrollRef.current) rulerScrollRef.current.scrollLeft = tracks.scrollLeft
    if (trackLabelsScrollRef.current) trackLabelsScrollRef.current.scrollTop = tracks.scrollTop
  }, [])
  useEffect(() => {
    const el = timelineRef.current
    if (!el) return
    let pending = 0, raf: number | null = null
    const onWheel = (e: WheelEvent) => {
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault()
        const d = e.deltaMode === 1 ? e.deltaY * 16 : e.deltaY
        pending += d
        if (raf === null) raf = requestAnimationFrame(() => {
          const capped = Math.sign(pending) * Math.min(Math.abs(pending), 30)
          const factor = Math.exp(-capped / 300)
          setZoomLevel((z: number) => Math.max(minZoom, Math.min(STUDIO_ZOOM_MAX, z * factor)))
          pending = 0; raf = null
        })
        return
      }
      const tracks = tracksScrollRef.current
      if (!tracks) return
      const isH = e.shiftKey || Math.abs(e.deltaX) > Math.abs(e.deltaY)
      e.preventDefault()
      if (isH) {
        const raw = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY
        tracks.scrollLeft += Math.sign(raw) * Math.min(Math.abs(raw), 80)
      } else tracks.scrollTop += e.deltaY
      syncFollowers()
    }
    el.addEventListener('wheel', onWheel, { passive: false, capture: true })
    return () => { el.removeEventListener('wheel', onWheel, { capture: true }); if (raf) cancelAnimationFrame(raf) }
  }, [minZoom, setZoomLevel, syncFollowers])
  useEffect(() => {
    const v = videoRef.current
    if (!v || !preview) return
    const onTime = () => setPlayheadMs(Math.round(v.currentTime * 1000))
    v.addEventListener('timeupdate', onTime)
    return () => v.removeEventListener('timeupdate', onTime)
  }, [preview, setPlayheadMs])
  const activeLiveCue = useMemo(() => {
    if (!mash) return null
    for (const t of mash.tracks.filter(x => x.kind === 'text')) {
      const cue = t.clips.find(c => playheadMs >= c.timelineStartMs && playheadMs < c.timelineEndMs)
      if (cue) return cue
    }
    return null
  }, [mash, playheadMs])
  const activeCueText = activeLiveCue?.text ?? ''
  const activeCueStyleKey = useMemo(() => {
    const tid = (activeLiveCue?.templateId ?? '') as string
    if (tid === 'subtitle_douyin') return 'sub-douyin'
    if (tid === 'subtitle_variety') return 'sub-variety'
    if (tid === 'subtitle_newsbar') return 'sub-news'
    if (tid === 'subtitle_bubble') return 'sub-bubble'
    if (tid === 'subtitle_impact') return 'sub-impact'
    if (tid === 'subtitle_karaoke') return 'sub-karaoke'
    return 'sub-classic'
  }, [activeLiveCue])
  const liveTextTracks = useMemo(() => {
    if (!mash) return timeline?.textTracks ?? []
    const tracks = mash.tracks.filter(t => t.kind === 'text')
    if (tracks.length === 0) return []
    const roleFromLabel = (label: string): string => {
      if (label === '标题') return 'headline'
      if (label === '标注') return 'callout'
      if (label === 'CTA') return 'cta'
      return 'subtitle'
    }
    return tracks.map(t => {
      const orig = timeline?.textTracks.find(x => x.id === t.id)
      return {
      id: t.id,
      role: ((t.clips[0]?.role as string | undefined) ?? roleFromLabel(t.label)) as import('../lib/local-store').TextTrack['role'],
      layer: orig?.layer ?? 1,
      enabled: t.enabled,
      origin: (t as unknown as { origin?: string }).origin ?? orig?.origin ?? 'storyboard_generated',
      generationId: (orig?.generationId ?? null) as string | null,
      editable: (t as unknown as { editable?: boolean }).editable ?? orig?.editable ?? true,
      locked: (t as unknown as { locked?: boolean }).locked ?? orig?.locked ?? false,
      cues: t.clips.map(c => ({
        id: c.id,
        templateId: c.templateId ?? null,
        startMs: c.timelineStartMs,
        endMs: c.timelineEndMs,
        text: c.text ?? '',
        style: c.textStyle ?? { fontKey:'jianying_default', fontSize:0.055, bold:true, color:'#FFFFFF', strokeColor:null, strokeWidth:0, shadow:false, backgroundColor:null, alignment:'center', letterSpacing:0, lineSpacing:0 },
        layout: c.textLayout ?? { anchor:'bottom', x:0.5, y:0.82, maxWidth:0.86, safeArea:'title_safe' },
        entrance: c.textEntrance ?? null,
        exit: c.textExit ?? null,
        loopAnimation: c.textLoop ?? null,
        jianyingCompatibility: 'verified' as const,
      })),
    }}) as unknown as import('../lib/local-store').TextTrack[]
  }, [mash, timeline])
  const liveSpanStyle = useMemo(() => {
    if (!activeLiveCue) return null as unknown as React.CSSProperties
    const s = (activeLiveCue.textStyle ?? { fontKey:'jianying_default', fontSize:0.055, bold:true, color:'#FFFFFF', strokeColor:null, strokeWidth:0, shadow:false, backgroundColor:null, alignment:'center', letterSpacing:0, lineSpacing:0 }) as { fontKey:string; fontSize:number; bold:boolean; color:string; strokeColor:string|null; strokeWidth:number; shadow:boolean; backgroundColor:string|null; alignment:string; letterSpacing:number; lineSpacing:number }
    const sizePx = Math.round(Math.max(12, s.fontSize * 400))
    const stroke = s.strokeColor && s.strokeWidth > 0 ? `${s.strokeWidth}px ${s.strokeColor}` : undefined
    const out: React.CSSProperties = {
      color: s.color,
      fontSize: sizePx,
      fontWeight: s.bold ? 800 : 600,
      letterSpacing: s.letterSpacing ? `${s.letterSpacing}px` : undefined,
      lineHeight: s.lineSpacing ? `${1.1 + s.lineSpacing * 0.04}` : 1.15,
      textAlign: (s.alignment as never) ?? 'center',
      backgroundColor: s.backgroundColor ?? undefined,
      padding: s.backgroundColor ? '4px 10px' : '2px 6px',
      borderRadius: s.backgroundColor ? 8 : undefined,
      textShadow: s.shadow ? '0 2px 10px rgba(0,0,0,0.7), 0 1px 2px rgba(0,0,0,0.9)' : undefined,
      display: 'inline-block',
      maxWidth: '92%',
      wordBreak: 'break-word' as never,
    }
    if (stroke) (out as unknown as Record<string,string>).WebkitTextStroke = stroke
    if (stroke) (out as unknown as Record<string,string>).paintOrder = 'stroke fill'
    return out
  }, [activeLiveCue])
  const liveWrapStyle = useMemo(() => {
    if (!activeLiveCue) return null as unknown as React.CSSProperties
    const l = (activeLiveCue.textLayout ?? { anchor:'bottom', x:0.5, y:0.82, maxWidth:0.86, safeArea:'title_safe' }) as { anchor:string; x:number; y:number; maxWidth:number }
    const anchor = l.anchor
    const map: Record<string,string> = { top:'translate(-50%,0)', middle_center:'translate(-50%,-50%)', center:'translate(-50%,-50%)', bottom:'translate(-50%,-100%)', bottom_left:'translate(0,-100%)', bottom_right:'translate(-100%,-100%)', top_left:'translate(0,0)', top_right:'translate(-100%,0)' }
    const tr = map[anchor] ?? 'translate(-50%,-100%)'
    return {
      position: 'absolute' as const,
      left: `${Math.max(0, Math.min(1, l.x)) * 100}%`,
      top: `${Math.max(0, Math.min(1, l.y)) * 100}%`,
      transform: tr,
      maxWidth: `${Math.max(0.5, Math.min(1, l.maxWidth)) * 100}%`,
      width: 'max-content' as never,
      textAlign: ((activeLiveCue.textStyle as unknown as { alignment?:string })?.alignment ?? 'center') as never,
      pointerEvents: 'none' as never,
    }
  }, [activeLiveCue])
  useEffect(() => {
    const v = videoRef.current
    const c = jassubContainerRef.current
    if (!v || !c) return
    let cancelled = false
    jassubHandleRef.current?.destroy()
    jassubHandleRef.current = null
    if (!liveTextTracks || liveTextTracks.length === 0 || liveTextTracks.every(t => t.cues.length === 0)) return
    const timer = setTimeout(() => {
      void attachJassub(v, c, liveTextTracks).then(h => {
        if (cancelled) { h?.destroy(); return }
        jassubHandleRef.current = h
      })
    }, 120)
    return () => { cancelled = true; clearTimeout(timer); jassubHandleRef.current?.destroy(); jassubHandleRef.current = null }
  }, [liveTextTracks])
  const seekToMs = useCallback((ms: number) => {
    const clamped = Math.max(0, Math.min(durationMs, ms))
    setPlayheadMs(clamped)
    const v = videoRef.current
    if (v && preview) v.currentTime = clamped / 1000
  }, [durationMs, preview, setPlayheadMs])
  useEffect(() => {
    if (!mash) { setThumbMap({}); prevAssetKeyRef.current = ''; return }
    const ids = [...new Set(mash.tracks.flatMap(t => (t.kind === 'video' || t.kind === 'overlay') ? t.clips.map(c => c.assetId).filter(Boolean) as string[] : []))]
    ids.sort()
    const key = ids.join(',')
    if (key === prevAssetKeyRef.current) return
    prevAssetKeyRef.current = key
    if (ids.length === 0) { setThumbMap({}); return }
    let cancelled = false
    Promise.all(ids.map(async id => {
      try {
        const ev = await getAssetEvidence(id)
        const kf = ev.keyframes[0]?.imagePath ?? null
        if (!kf) return null
        return [id, convertFileSrc(kf)] as const
      } catch { return null }
    })).then(results => {
      if (cancelled) return
      const next: Record<string, string> = {}
      for (const r of results) if (r) next[r[0]] = r[1]
      setThumbMap(next)
    })
    return () => { cancelled = true }
  }, [mash])
  useEffect(() => {
    if (!pickerOpen || !projectId) return
    let cancelled = false
    void listAssetPage(projectId, { search: pickerQuery || undefined, offset: 0, limit: 30 }).then(page => {
      if (cancelled) return
      setPickerAssets(page.items)
    }).catch(() => { if (!cancelled) setPickerAssets([]) })
    return () => { cancelled = true }
  }, [pickerOpen, projectId, pickerQuery])
  useEffect(() => {
    const onClick = () => setContextMenu(null)
    window.addEventListener('click', onClick)
    return () => window.removeEventListener('click', onClick)
  }, [])
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null
      if (target && (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA' || target.isContentEditable)) return
      if (e.key === 'Delete' || e.key === 'Backspace') {
        if (controller.deleteSelected()) e.preventDefault()
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'a') {
        e.preventDefault()
        if (mash) {
          const all = mash.tracks.flatMap(t => t.clips.map(c => c.id))
          controller.setSelectedClipIds(all)
        }
      } else if (e.code === 'Space' && !e.ctrlKey && !e.metaKey) {
        e.preventDefault()
        const v = videoRef.current
        if (v) { if (v.paused) void v.play(); else v.pause() }
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z') {
        e.preventDefault()
        if (e.shiftKey) controller.redo(); else controller.undo()
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'y') {
        e.preventDefault(); controller.redo()
      } else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'd') {
        e.preventDefault(); controller.duplicateSelected()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [controller, mash])
  const handleRulerMouseDown = useCallback((e: React.MouseEvent) => {
    const rect = (e.currentTarget as HTMLDivElement).getBoundingClientRect()
    const scrollLeft = rulerScrollRef.current?.scrollLeft ?? 0
    const x = e.clientX - rect.left + scrollLeft
    seekToMs(x / scale)
    setScrubbing(true)
    const onMove = (ev: MouseEvent) => {
      const r = (e.currentTarget as HTMLDivElement).getBoundingClientRect()
      const sl = rulerScrollRef.current?.scrollLeft ?? 0
      seekToMs((ev.clientX - r.left + sl) / scale)
    }
    const onUp = () => { setScrubbing(false); window.removeEventListener('mousemove', onMove); window.removeEventListener('mouseup', onUp) }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }, [scale, seekToMs])
  const isOverlayId = (id: string) => id.startsWith('overlay-')
  const handleClipPointerDown = useCallback((e: React.PointerEvent, clipId: string, side: 'body' | 'left' | 'right') => {
    const owningTrack = mash?.tracks.find(t => t.clips.some(c => c.id === clipId))
    if (owningTrack && controller.isTrackLocked(owningTrack.id)) return
    e.stopPropagation()
    setContextMenu(null)
    const additive = e.ctrlKey || e.metaKey
    if (side === 'body') {
      if (additive) controller.setSelectedClipId(clipId, { additive: true })
      else if (!controller.isSelected(clipId)) controller.setSelectedClipId(clipId)
    } else {
      controller.setSelectedClipId(clipId)
    }
    const clip = mash?.tracks.flatMap(t => t.clips).find(c => c.id === clipId)
    if (!clip) return
    controller.pushHistory()
    const startX = e.clientX
    const startMs = clip.timelineStartMs
    const startDur = clip.timelineEndMs - clip.timelineStartMs
    if (side === 'body') setDrag({ type: 'move', clipId, startX, startMs, startDur })
    else setDrag({ type: side === 'left' ? 'resize-left' : 'resize-right', clipId, startX, startMs, startDur })
    ;(e.currentTarget as Element).setPointerCapture(e.pointerId)
  }, [controller, mash])
  const handleClipContextMenu = useCallback((e: React.MouseEvent, clipId: string) => {
    e.preventDefault()
    e.stopPropagation()
    if (!controller.isSelected(clipId)) controller.setSelectedClipId(clipId)
    setContextMenu({ x: e.clientX, y: e.clientY, clipId })
  }, [controller])
  const handleTracksPointerDown = useCallback((e: React.PointerEvent) => {
    const target = e.target as HTMLElement
    if (target.closest('.timeline-element') || target.closest('.timeline-playhead') || target.closest('.ruler')) return
    const rect = (tracksScrollRef.current as HTMLDivElement | null)?.getBoundingClientRect()
    if (!rect) return
    const startX = e.clientX - rect.left + (tracksScrollRef.current?.scrollLeft ?? 0)
    const startY = e.clientY - rect.top + (tracksScrollRef.current?.scrollTop ?? 0)
    setMarquee({ startX, curX: startX, startY, curY: startY })
    const onMove = (ev: MouseEvent) => {
      const r = tracksScrollRef.current?.getBoundingClientRect()
      if (!r) return
      const curX = ev.clientX - r.left + (tracksScrollRef.current?.scrollLeft ?? 0)
      const curY = ev.clientY - r.top + (tracksScrollRef.current?.scrollTop ?? 0)
      setMarquee(m => m ? { ...m, curX, curY } : null)
    }
    const onUp = (ev: MouseEvent) => {
      window.removeEventListener('mousemove', onMove)
      window.removeEventListener('mouseup', onUp)
      setMarquee(m => {
        if (!m) return null
        const r = tracksScrollRef.current?.getBoundingClientRect()
        if (!r) return null
        const curX = ev.clientX - r.left + (tracksScrollRef.current?.scrollLeft ?? 0)
        const lo = Math.min(m.startX, curX), hi = Math.max(m.startX, curX)
        const moved = Math.abs(hi - lo) > 6
        if (moved) {
          const loMs = lo / scale
          const hiMs = hi / scale
          const additive = ev.ctrlKey || ev.metaKey || ev.shiftKey
          controller.selectClipsInRange(loMs, hiMs, additive)
        } else if (!ev.ctrlKey && !ev.metaKey && !ev.shiftKey) {
          controller.clearSelection()
        }
        return null
      })
    }
    window.addEventListener('mousemove', onMove)
    window.addEventListener('mouseup', onUp)
  }, [controller, scale])
  const isOverlaySelected = selectedClipId ? isOverlayId(selectedClipId) : false
  const handlePointerMove = useCallback((e: React.PointerEvent) => {
    if (!drag || !mash) return
    const deltaPx = e.clientX - drag.startX
    const deltaMs = deltaPx / scale
    const isOverlayDrag = isOverlayId(drag.clipId)
    if (drag.type === 'resize-right') {
      const nextDur = Math.max(200, Math.min(12000, drag.startDur + deltaMs))
      if (isOverlayDrag) controller.updateOverlayClipDurationTransient(drag.clipId, nextDur)
      else controller.updateClipDurationTransient(drag.clipId, nextDur)
      if (snapEnabled) {
        const edges: number[] = []
        for (const t of mash.tracks) {
          if (controller.isTrackHidden(t.id)) continue
          for (const c of t.clips) if (c.id !== drag.clipId) { edges.push(c.timelineStartMs, c.timelineEndMs) }
        }
        const clip = mash.tracks.flatMap(t=>t.clips).find(c=>c.id===drag.clipId)
        if (clip) {
          const edge = clip.timelineStartMs + nextDur
          let best: number | null = null, bestDist = 8/scale + 1
          for (const ex of edges) { const d = Math.abs(ex - edge); if (d < bestDist) { bestDist = d; best = ex } }
          setSnapX(best !== null && bestDist*scale < 8 ? best*scale : null)
        }
      } else setSnapX(null)
    } else if (drag.type === 'resize-left') {
      if (isOverlayDrag) controller.resizeOverlayLeftTransient(drag.clipId, drag.startMs + deltaMs)
      else controller.resizeClipLeftTransient(drag.clipId, deltaMs)
      setSnapX(null)
    } else if (drag.type === 'move') {
      if (isOverlayDrag) {
        controller.moveOverlayClipTransient(drag.clipId, drag.startMs + deltaMs)
        if (snapEnabled) {
          const edges: number[] = []
          for (const t of mash.tracks) for (const c of t.clips) if (c.id !== drag.clipId) { edges.push(c.timelineStartMs, c.timelineEndMs) }
          let best: number | null = null, bestDist = 8/scale + 1
          const curStart = drag.startMs + deltaMs
          for (const ex of edges) { const d = Math.abs(ex - curStart); if (d < bestDist) { bestDist = d; best = ex } }
          setSnapX(best !== null && bestDist*scale < 8 ? best*scale : null)
        } else setSnapX(null)
        return
      }
      const track = mash.tracks.find(t => t.kind === 'video')
      if (!track) return
      const centerMs = drag.startMs + deltaMs + drag.startDur / 2
      let targetIdx = 0
      for (let i = 0; i < track.clips.length; i += 1) {
        if (track.clips[i].timelineStartMs < centerMs) targetIdx = i
      }
      const fromIdx = track.clips.findIndex(c => c.id === drag.clipId)
      if (targetIdx !== fromIdx) setSnapX(track.clips[targetIdx].timelineStartMs * scale)
      else setSnapX(null)
    }
  }, [drag, mash, scale, snapEnabled, controller])
  const handlePointerUp = useCallback((e: React.PointerEvent) => {
    if (!drag || !mash) { setDrag(null); setSnapX(null); return }
    if (drag.type === 'move') {
      if (isOverlayId(drag.clipId)) {
        if (snapEnabled && snapX !== null) {
          const targetMs = snapX / scale
          controller.moveOverlayClipTransient(drag.clipId, targetMs)
        }
        controller.setCommitState('idle'); setDrag(null); setSnapX(null); return
      }
      const deltaMs = (e.clientX - drag.startX) / scale
      const centerMs = drag.startMs + deltaMs + drag.startDur / 2
      const track = mash.tracks.find(t => t.kind === 'video')
      if (track) {
        const fromIdx = track.clips.findIndex(c => c.id === drag.clipId)
        let targetIdx = 0
        for (let i = 0; i < track.clips.length; i += 1) if (track.clips[i].timelineStartMs < centerMs) targetIdx = i
        if (fromIdx !== targetIdx) {
          controller.reorderClipsWithoutHistory(fromIdx, targetIdx)
        }
      }
    } else {
      controller.setCommitState('idle')
    }
    setDrag(null); setSnapX(null)
  }, [drag, mash, scale, controller, snapEnabled, snapX])
  const diffHint = diff && hasChanges ? [
    diff.orderChanged ? '顺序已调整' : null,
    diff.durationsChanged ? `${diff.durationAdjustments.length}段时长` : null,
    (diff as unknown as { clipReplacements?: unknown[] }).clipReplacements && (diff as unknown as { clipReplacements: unknown[] }).clipReplacements.length ? `${(diff as unknown as { clipReplacements: unknown[] }).clipReplacements.length}段替换` : null,
    diff.inserted.length ? `${diff.inserted.length}段新增(分割/复制)` : null,
    diff.deletedShotIndices.length ? `${diff.deletedShotIndices.length}段已删除` : null,
    diff.overlayChanged ? `${diff.overlayInserted.length}叠加新增/${diff.overlayDeletedShotIndices.length}删除/${diff.overlayDurationAdjustments.length}调整` : null,
    diff.audioChanged ? `音频${diff.audioVolumeAdjustments.length}处音量` : null,
    diff.textChanged ? '字幕已改' : null,
  ].filter(Boolean).join(' · ') : ''
  const canCommit = hasChanges && controller.commitState !== 'committing' && !!projectId && !!sessionId
  async function handleCommit() {
    if (!timeline || !projectId || !sessionId || !diff || !hasChanges) return
    controller.setCommitState('committing'); controller.setCommitError(null)
    try {
      const d = diff as unknown as { clipReplacements?: Array<{ shotIndex:number; assetId:string; sourceStartMs:number; sourceEndMs:number }> }
      const result = await commitStudioEdits({
        projectId, editingTaskId: sessionId, timelineVersionId: timeline.id,
        reorder: diff.newOrder ?? undefined,
        adjustments: diff.durationAdjustments.length ? diff.durationAdjustments : undefined,
        clipReplacements: d.clipReplacements?.length ? d.clipReplacements : undefined,
        textTracks: diff.updatedTextTracks ?? undefined,
        inserted: diff.inserted.length ? diff.inserted : undefined,
        deletedShotIndices: diff.deletedShotIndices.length ? diff.deletedShotIndices : undefined,
        overlayInserted: diff.overlayInserted.length ? diff.overlayInserted.map(c => ({ assetId: c.assetId, sourceStartMs: c.sourceStartMs, sourceEndMs: c.sourceEndMs, timelineStartMs: c.timelineStartMs, timelineEndMs: c.timelineEndMs, onScreenText: c.onScreenText })) : undefined,
        overlayDeletedShotIndices: diff.overlayDeletedShotIndices.length ? diff.overlayDeletedShotIndices : undefined,
        overlayAdjustments: diff.overlayDurationAdjustments.length ? diff.overlayDurationAdjustments : undefined,
        overlayReorder: diff.overlayNewOrder ?? undefined,
        musicTracks: diff.updatedMusicTracks ?? undefined,
        voiceoverTracks: diff.updatedVoiceoverTracks ?? undefined,
      })
      controller.setCommitState('success')
      onCommitted(result.timeline, null)
      setRenderingPreview(true)
      const timeout = (ms: number) => new Promise<never>((_, rej) => { setTimeout(() => rej(new Error(`预览生成超时(${ms/1000}s)，版本已保存，预览将在后台继续`)), ms) })
      void Promise.race([renderPreview(result.timeline.id), timeout(60000)])
        .then(p => { onCommitted(result.timeline, p as PreviewResult); setRenderingPreview(false) })
        .catch(err => { controller.setCommitError(err instanceof Error ? err.message : String(err)); setRenderingPreview(false) })
    } catch (err) { controller.setCommitState('error'); controller.setCommitError(err instanceof Error ? err.message : String(err)) }
  }
  if (!timeline || !mash) {
    return (
      <section className="studio-v2 studio-v2--empty">
        <div className="studio-empty">
          <span className="eyebrow">STUDIO · OPENCUT</span>
          <h2>还没有可编辑的时间线</h2>
          <p>先在「Agent」生成故事板，系统会自动创建内部时间线与预览，之后即可在此像剪映一样拖拽微调。</p>
        </div>
      </section>
    )
  }
  const videoTrack = mash.tracks.find(t => t.kind === 'video')
  const selected = selectedClipId ? mash.tracks.flatMap(t => t.clips).find(c => c.id === selectedClipId) ?? null : null
  const selectedIsAudio = selected?.kind === 'audio'
  const rulerCfg = getOpencutRulerConfig({ zoomLevel, fps: FPS_30 })
  const labelInterval = rulerCfg.labelIntervalSeconds
  const tickInterval = rulerCfg.tickIntervalSeconds
  const canSplit = (() => {
    if (!mash) return false
    const vt = mash.tracks.find(t => t.kind === 'video')
    if (!vt) return false
    const target = selectedClipId ? vt.clips.find(c => c.id === selectedClipId) : null
    const clip = target && playheadMs > target.timelineStartMs && playheadMs < target.timelineEndMs ? target : vt.clips.find(c => playheadMs > c.timelineStartMs && playheadMs < c.timelineEndMs)
    if (!clip) return false
    const left = playheadMs - clip.timelineStartMs, right = clip.timelineEndMs - playheadMs
    return left >= 200 && right >= 200 && left >= 120 && right >= 120
  })()
  return (
    <section className="studio-v2" onPointerMove={handlePointerMove} onPointerUp={handlePointerUp} onClick={() => setContextMenu(null)}>
      <div className="studio-v2__header">
        <div className="studio-v2__header-left">
          <span className="eyebrow">STUDIO · OPENCUT CLASSIC · 全面版</span>
          <strong>剪映工作台</strong>
          <small>v{timeline.versionNumber} · {formatMs(durationMs)} · {videoTrack?.clips.length ?? 0}段{(mash.tracks.find(t=>t.kind==='overlay')?.clips.length ?? 0) ? ` · 叠加 ${mash.tracks.find(t=>t.kind==='overlay')!.clips.length}段` : ''}{selectedClipIds.length>1 ? ` · 已选 ${selectedClipIds.length}` : ''}</small>
        </div>
        <div className="studio-v2__header-right">
          <button className="studio-icon-btn" disabled={!canUndo} onClick={() => controller.undo()} title="撤销 Ctrl+Z">↩ 撤销</button>
          <button className="studio-icon-btn" disabled={!canRedo} onClick={() => controller.redo()} title="重做 Ctrl+Shift+Z">↪ 重做</button>
          <span className="studio-sep" />
          <button className={`studio-icon-btn ${snapEnabled ? 'is-active' : ''}`} onClick={() => setSnapEnabled(v => !v)} title="磁吸">🧲 磁吸</button>
          <button className={`studio-icon-btn ${rippleEnabled ? 'is-active' : ''}`} onClick={() => setRippleEnabled(v => !v)} title="波纹">〰 波纹</button>
          <div className="studio-zoom">
            <button className="studio-icon-btn" onClick={() => setZoomLevel(z => Math.max(minZoom, z / 1.25))}>－</button>
            <input type="range" min={0} max={1} step={0.01} value={zoomToSlider(zoomLevel, minZoom)} onChange={e => setZoomLevel(sliderToZoom(Number(e.target.value), minZoom))} />
            <button className="studio-icon-btn" onClick={() => setZoomLevel(z => Math.min(STUDIO_ZOOM_MAX, z * 1.25))}>＋</button>
            <span>{Math.round(zoomLevel * 100)}%</span>
          </div>
          <button className="outline-button" onClick={controller.reset} disabled={!hasChanges && !canUndo}>重置</button>
          <button className="primary-button" onClick={() => void handleCommit()} disabled={!canCommit}>{controller.commitState === 'committing' ? '保存中…' : renderingPreview ? '预览渲染中…' : hasChanges ? '保存为新版本' : '无改动'}</button>
        </div>
      </div>
      {controller.commitError && <p className="studio-error">{controller.commitError}</p>}
      {controller.commitState === 'success' && <p className="studio-success">已保存为新版本{renderingPreview ? '，预览渲染中…' : '，预览已刷新。'}</p>}
      {diff && hasChanges && <p className="studio-diff-hint">{diffHint}</p>}
      <div className="studio-v2__body">
        <div className="studio-v2__preview">
          <div className="preview-viewport" style={{position:'relative'}}>
            {preview ? (
              <>
                <video ref={videoRef} controls playsInline src={`${convertFileSrc(preview.previewPath)}?v=${previewNonce}`} onClick={() => videoRef.current?.paused ? void videoRef.current?.play() : videoRef.current?.pause()} />
                <div ref={jassubContainerRef} style={{position:'absolute', inset:0, pointerEvents:'none'}} />
                {activeLiveCue && <div style={liveWrapStyle as React.CSSProperties}><span style={liveSpanStyle as React.CSSProperties} className={activeCueStyleKey}>{activeCueText}</span></div>}
              </>
            ) : (
              <div className="preview-placeholder" style={{position:'relative'}}>
                <span>暂无预览</span><small>保存后生成 540×960 本地预览</small>
                {activeLiveCue && <div style={liveWrapStyle as React.CSSProperties}><span style={liveSpanStyle as React.CSSProperties} className={activeCueStyleKey}>{activeCueText}</span></div>}
              </div>
            )}
            <div className="preview-overlay">
              <span>{formatMs(playheadMs)} / {formatMs(durationMs)}</span>
              <span>{preview ? 'LOCAL PREVIEW + jassub' : 'NO PREVIEW · 花字CSS预览'}</span>
            </div>
          </div>
          <div className="preview-toolbar">
            <button className="studio-icon-btn" onClick={() => videoRef.current && (videoRef.current.currentTime = Math.max(0, videoRef.current.currentTime - 0.1))}>⏪ -0.1s</button>
            <button className="primary-button preview-play" onClick={() => videoRef.current && (videoRef.current.paused ? void videoRef.current.play() : videoRef.current.pause())}>{videoRef.current?.paused === false ? '❚❚ 暂停' : '▶ 播放'}</button>
            <button className="studio-icon-btn" onClick={() => videoRef.current && (videoRef.current.currentTime = Math.min(durationMs / 1000, videoRef.current.currentTime + 0.1))}>+0.1s ⏩</button>
            <small>{hasChanges ? '· 有未保存改动' : '· 已同步'}{selectedClipIds.length>1 ? ` · 已选 ${selectedClipIds.length}` : ''}</small>
          </div>
        </div>
        <div className="studio-v2__inspector" style={{overflowY:'auto'}}>
          <div className="inspector-head"><strong>属性</strong><small>{selected ? `${selected.id}${selectedClipIds.length>1 ? ` +${selectedClipIds.length-1}` : ''}` : '未选中'}</small></div>
          {!selected ? <p className="inspector-empty">点选/框选/右键片段以编辑。Ctrl+点 多选，拖空白 框选。<button className="outline-button" style={{marginTop:8}} onClick={()=>controller.addOverlayAtPlayhead()}>＋ 在播放头添加叠加</button></p>
            : selected.kind === 'video' ? <VideoInspector clip={selected} controller={controller} mash={mash} multi={selectedClipIds.length>1} onPick={(id)=>{ setPickerTarget(id); setPickerOpen(true)}} />
            : selected.kind === 'overlay' ? <OverlayInspector clip={selected} controller={controller} onPick={(id)=>{ setPickerTarget(id); setPickerOpen(true)}} />
            : selectedIsAudio ? <AudioInspector clip={selected} controller={controller} />
            : <TextInspector clip={selected} controller={controller} />}
        </div>
      </div>
      <div className="studio-v2__timeline panel" ref={timelineRef}>
        <div className="timeline-toolbar">
          <div className="timeline-toolbar__left">
            <button className="studio-icon-btn" disabled={!canSplit} onClick={() => controller.splitClipAtPlayhead()} title="在播放头处分割">✂ 分割</button>
            <button className="studio-icon-btn" disabled={!selected} onClick={() => controller.duplicateSelected()} title="复制">⧉ 复制</button>
            <button className="studio-icon-btn" disabled={!selected} onClick={() => controller.deleteSelected()} title="删除">🗑 删除{selectedClipIds.length>1 ? `(${selectedClipIds.length})` : ''}</button>
            <button className="studio-icon-btn" onClick={() => controller.addOverlayAtPlayhead()} title="在播放头添加叠加">⬢ 叠加</button>
            <span className="timeline-toolbar__sep" />
            <span className="timeline-toolbar__hint">{isOverlaySelected ? '叠加：拖动定位 · 右柄改时长' : selectedIsAudio ? '音频：音量/淡入淡出/循环 已全面' : '视频：拖动重排/两端改时长/替换素材/源窗口 已全面'}</span>
          </div>
          <div className="timeline-toolbar__right">
            <span>{formatMs(playheadMs)}</span>
          </div>
        </div>
        <div className="timeline-body">
          <div className="track-labels-col" ref={trackLabelsScrollRef as never}>
            <div style={{ height: RULER_H }} />
            <div className="track-labels" style={{ gap: GAP }}>
              {mash.tracks.map(track => {
                const muted = controller.isTrackMuted(track.id)
                const hidden = controller.isTrackHidden(track.id)
                const locked = controller.isTrackLocked(track.id)
                return (
                  <div key={track.id} className={`track-label ${hidden ? 'is-hidden' : ''} ${locked ? 'is-locked' : ''} ${muted ? 'is-muted' : ''}`} style={{ height: TRACK_H[track.kind] ?? 50 }} title={`${track.label} · ${track.clips.length}段`}>
                    <span className="track-label__icon">{track.kind === 'video' ? '🎬' : track.kind === 'overlay' ? '⬢' : track.kind === 'text' ? 'Aa' : '♪'}</span>
                    <span className="track-label__name">{track.label}</span>
                    <span className="track-label__count">{track.clips.length}</span>
                    <div className="track-label__actions" onPointerDown={e => e.stopPropagation()}>
                      <button className={`track-action ${muted ? 'is-active' : ''}`} onClick={() => controller.toggleTrackMute(track.id)} title="静音">{muted ? '🔇' : '🔊'}</button>
                      <button className={`track-action ${hidden ? 'is-active' : ''}`} onClick={() => controller.toggleTrackHidden(track.id)} title="隐藏">{hidden ? '🚫' : '👁'}</button>
                      <button className={`track-action ${locked ? 'is-active' : ''}`} onClick={() => controller.toggleTrackLocked(track.id)} title="锁定">{locked ? '🔒' : '🔓'}</button>
                    </div>
                  </div>
                )
              })}
            </div>
            <div style={{ height: 12 }} />
          </div>
          <div className="tracks-col" ref={tracksScrollRef as never} onScroll={syncFollowers} onPointerDown={handleTracksPointerDown}>
            <div style={{ width: contentW, position:'relative' }}>
              <div ref={rulerScrollRef as never} className="ruler-scroll" style={{ height: RULER_H }}>
                <div className="ruler" style={{ width: contentW, height: RULER_H }} onMouseDown={handleRulerMouseDown}>
                  {rulerTicks(durationMs, tickInterval, labelInterval, pps).map(t => (
                    <span key={t.s} className={`ruler-tick ${t.label ? 'is-label' : ''}`} style={{ left: t.s * pps }}>
                      <i style={{ height: t.label ? 10 : 5 }} />
                      {t.label && <em>{fmtRulerLabel(t.s)}</em>}
                    </span>
                  ))}
                  <div className="ruler-playhead" style={{ left: playheadMs * scale }} />
                </div>
              </div>
              <div className="tracks" style={{ gap: GAP, paddingTop: PADDING_TOP, width: contentW }}>
                {mash.tracks.map(track => {
                  const hidden = controller.isTrackHidden(track.id)
                  const muted = controller.isTrackMuted(track.id)
                  const locked = controller.isTrackLocked(track.id)
                  return (
                  <div key={track.id} className={`track-row ${hidden ? 'is-hidden' : ''} ${muted ? 'is-muted' : ''} ${locked ? 'is-locked' : ''}`} style={{ height: TRACK_H[track.kind] ?? 50 }}>
                    <div className="track-lane">
                      {track.clips.map((clip, idx) => {
                        const left = clip.timelineStartMs * scale
                        const width = Math.max(12, (clip.timelineEndMs - clip.timelineStartMs) * scale)
                        const sel = controller.isSelected(clip.id)
                        const isDragging = drag?.clipId === clip.id
                        return (
                          <div
                            key={clip.id}
                            className={`timeline-element ${track.kind} ${sel ? 'is-selected' : ''} ${isDragging ? 'is-dragging' : ''} ${hidden ? 'is-track-hidden' : ''} ${selectedClipIds.length>1 && sel ? 'is-multi' : ''}`}
                            style={{ left, width, height: TRACK_H[track.kind] ?? 50 }}
                            onPointerDown={e => handleClipPointerDown(e, clip.id, 'body')}
                            onContextMenu={e => handleClipContextMenu(e, clip.id)}
                            onClick={e => { if (e.ctrlKey || e.metaKey) { e.stopPropagation(); controller.setSelectedClipId(clip.id, { additive: true }) } }}
                            title={`${clip.id} · ${formatMs(clip.timelineEndMs - clip.timelineStartMs)}`}
                          >
                            <div className="timeline-element__inner">
                              {track.kind === 'video' ? (
                                <div className="element-content video">
                                  <div className="tiled-thumb" style={clip.assetId && thumbMap[clip.assetId] ? { backgroundImage: `url(${thumbMap[clip.assetId]})`, backgroundRepeat: 'repeat-x', backgroundSize: `${Math.round(TRACK_H.video * 16 / 9)}px ${TRACK_H.video}px`, backgroundPosition: 'left center', opacity: 0.92 } : undefined} />
                                  <span className="element-title">{clip.text ? clip.text.slice(0, 16) : `镜头 ${idx + 1}`}</span>
                                  <span className="element-sub">{formatMs(clip.timelineEndMs - clip.timelineStartMs)}</span>
                                </div>
                              ) : track.kind === 'overlay' ? (
                                <div className="element-content overlay">
                                  <span className="element-title">⬢ {clip.text ? clip.text.slice(0, 12) : `叠加 ${idx + 1}`}</span>
                                  <span className="element-sub">{formatMs(clip.timelineStartMs)}→{formatMs(clip.timelineEndMs)}</span>
                                </div>
                              ) : track.kind === 'text' ? (
                                <div className="element-content text"><span>{clip.text?.slice(0, 20) ?? `字幕 ${idx + 1}`}</span></div>
                              ) : (
                                <div className="element-content audio">
                                  <div className="waveform-bars" aria-hidden>
                                    {waveformHeights(clip.id, width).map((h, i) => (
                                      <i key={i} style={{ height: `${h}%` }} />
                                    ))}
                                  </div>
                                  <span className="audio-label">{formatMs(clip.timelineEndMs - clip.timelineStartMs)}{clip.volume !== undefined ? ` · ${Math.round((clip.volume ?? 1)*100)}%` : ''}{clip.loopEnabled ? ' · ↻' : ''}</span>
                                </div>
                              )}
                            </div>
                            {sel && !controller.isTrackLocked(track.id) && (
                              <>
                                <button className="resize-handle left" onPointerDown={e => handleClipPointerDown(e, clip.id, 'left')} aria-label="左拉伸" />
                                <button className="resize-handle right" onPointerDown={e => handleClipPointerDown(e, clip.id, 'right')} aria-label="右拉伸" />
                              </>
                            )}
                          </div>
                        )
                      })}
                    </div>
                  </div>
                  )
                })}
                {marquee && (
                  <div style={{ position:'absolute', left: Math.min(marquee.startX, marquee.curX), width: Math.abs(marquee.curX - marquee.startX), top: RULER_H + PADDING_TOP, bottom: 0, border:'1px solid #d9ff62', background:'rgba(217,255,98,0.12)', pointerEvents:'none', zIndex: 6 }} />
                )}
                {snapX !== null && <div className="snap-indicator" style={{ left: snapX }} />}
                <div className="timeline-playhead" style={{ left: playheadMs * scale, height: `calc(100% - ${RULER_H}px)` }} onMouseDown={e => {
                  const startX = e.clientX, startMs = playheadMs
                  const onMove = (ev: MouseEvent) => seekToMs(startMs + (ev.clientX - startX) / scale)
                  const onUp = () => { window.removeEventListener('mousemove', onMove); window.removeEventListener('mouseup', onUp) }
                  window.addEventListener('mousemove', onMove); window.addEventListener('mouseup', onUp)
                }}>
                  <i className="playhead-knob" />
                  <span className="playhead-line" />
                </div>
                {scrubbing && <div className="timeline-playhead is-scrubbing" style={{ left: playheadMs * scale }}><span className="playhead-line" /></div>}
              </div>
            </div>
          </div>
        </div>
      </div>
      {contextMenu && (
        <div style={{ position:'fixed', left: contextMenu.x, top: contextMenu.y, background:'#1e1f1b', border:'1px solid #3a3c36', borderRadius:6, padding:6, display:'grid', gap:4, zIndex:50, minWidth:160, boxShadow:'0 10px 30px rgba(0,0,0,0.5)' }} onClick={e=>e.stopPropagation()}>
          <button className="outline-button" style={{textAlign:'left'}} onClick={()=>{ setContextMenu(null); controller.splitClipAtPlayhead() }} disabled={!canSplit}>✂ 在播放头分割</button>
          <button className="outline-button" style={{textAlign:'left'}} onClick={()=>{ setContextMenu(null); if(contextMenu.clipId) controller.duplicateClip(contextMenu.clipId) }}>⧉ 复制</button>
          <button className="outline-button" style={{textAlign:'left'}} onClick={()=>{ setContextMenu(null); controller.deleteSelected() }}>🗑 删除{selectedClipIds.length>1 ? ` (${selectedClipIds.length})` : ''}</button>
          <button className="outline-button" style={{textAlign:'left'}} onClick={()=>{ setContextMenu(null); controller.addOverlayAtPlayhead() }}>⬢ 在此添加叠加</button>
          <button className="outline-button" style={{textAlign:'left'}} onClick={()=>{ setContextMenu(null); controller.clearSelection() }}>清除选择</button>
        </div>
      )}
      {pickerOpen && (
        <div style={{position:'fixed', inset:0, background:'rgba(0,0,0,0.55)', zIndex:60, display:'flex', alignItems:'center', justifyContent:'center'}} onClick={()=>setPickerOpen(false)}>
          <div style={{background:'#1e1f1b', border:'1px solid #3a3c36', borderRadius:8, padding:16, minWidth:520, maxWidth:720, maxHeight:'80vh', overflow:'auto'}} onClick={e=>e.stopPropagation()}>
            <div style={{display:'flex', gap:8, marginBottom:12}}>
              <input placeholder="搜索素材名" value={pickerQuery} onChange={e=>setPickerQuery(e.target.value)} style={{flex:1, background:'#2a2c27', border:'1px solid #3a3c36', color:'#f0f0e8', padding:'6px 8px', borderRadius:4}} />
              <button className="outline-button" onClick={()=>setPickerOpen(false)}>关闭</button>
            </div>
            <div style={{display:'grid', gridTemplateColumns:'repeat(3,1fr)', gap:8}}>
              {pickerAssets.map(a=> (
                <button key={a.id} onClick={()=>{ if(pickerTarget) controller.replaceVideoAsset(pickerTarget, a.id); setPickerOpen(false)}} style={{textAlign:'left', background:'#2a2c27', border:'1px solid #3a3c36', borderRadius:6, padding:8, color:'#f0f0e8', cursor:'pointer'}}>
                  <div style={{fontSize:12, fontWeight:600, whiteSpace:'nowrap', overflow:'hidden', textOverflow:'ellipsis'}}>{a.displayName}</div>
                  <div style={{fontSize:10, opacity:0.6}}>{a.kind} · {a.durationMs ? formatMs(a.durationMs) : '—'} · {a.analysisStatus}</div>
                </button>
              ))}
              {pickerAssets.length===0 && <small style={{opacity:0.6, gridColumn:'1/4', textAlign:'center', padding:12}}>无匹配素材，请先在「素材」导入</small>}
            </div>
          </div>
        </div>
      )}
    </section>
  )
}

function VideoInspector({ clip, controller, mash, multi, onPick }: { clip: Mash['tracks'][number]['clips'][number]; controller: StudioWorkspaceController; mash: Mash; multi?: boolean; onPick: (clipId:string)=>void }) {
  const videoTrack = mash.tracks.find(t => t.kind === 'video')!
  const idx = videoTrack.clips.findIndex(c => c.id === clip.id)
  const dur = clip.timelineEndMs - clip.timelineStartMs
  return (
    <div className="inspector-body" style={{display:'grid', gap:10}}>
      <div style={{display:'grid', gap:6, padding:8, background:'#252721', borderRadius:6, border:'1px solid #3a3c36'}}>
        <strong style={{fontSize:12}}>素材</strong>
        <small style={{opacity:0.7, wordBreak:'break-all'}}>{clip.assetId ?? '—'}</small>
        <button className="outline-button" onClick={()=>onPick(clip.id)}>替换素材…</button>
        <label>源起点 ms <input type="number" value={clip.sourceStartMs} onChange={e=>controller.updateClipSourceWindow(clip.id, Number(e.target.value))} style={{width:'100%'}} /></label>
        <small style={{opacity:0.6}}>保存时校验源窗口≤文件时长；时长由时间轴手柄控制</small>
      </div>
      <label>时长 <input type="range" min={200} max={12000} step={100} value={dur} onChange={e => controller.updateClipDuration(clip.id, Number(e.target.value))} /><span>{dur}ms · {formatMs(dur)}</span></label>
      <div className="inspector-actions">
        <button className="outline-button" disabled={idx <= 0} onClick={() => controller.reorderClips(idx, idx - 1)}>上移</button>
        <button className="outline-button" disabled={idx >= videoTrack.clips.length - 1} onClick={() => controller.reorderClips(idx, idx + 1)}>下移</button>
        <button className="outline-button" onClick={() => controller.duplicateSelected()}>复制{multi ? `(${controller.selectedClipIds.length})` : ''}</button>
        <button className="outline-button" onClick={() => controller.deleteSelected()}>删除{multi ? `(${controller.selectedClipIds.length})` : ''}</button>
      </div>
    </div>
  )
}
function OverlayInspector({ clip, controller, onPick }: { clip: Mash['tracks'][number]['clips'][number]; controller: StudioWorkspaceController; onPick:(clipId:string)=>void }) {
  const dur = clip.timelineEndMs - clip.timelineStartMs
  return (
    <div className="inspector-body" style={{display:'grid', gap:10}}>
      <div style={{display:'grid', gap:6, padding:8, background:'#252721', borderRadius:6, border:'1px solid #3a3c36'}}>
        <strong style={{fontSize:12}}>叠加素材</strong>
        <small style={{opacity:0.7, wordBreak:'break-all'}}>{clip.assetId ?? '—'}</small>
        <button className="outline-button" onClick={()=>onPick(clip.id)}>替换素材…</button>
        <label>源起点 <input type="number" value={clip.sourceStartMs} onChange={e=>controller.updateClipSourceWindow(clip.id, Number(e.target.value))} style={{width:'100%'}}/></label>
      </div>
      <label>起点 <input type="range" min={0} max={Math.max(0, (controller.mash?.durationMs ?? 20000) - 200)} step={100} value={clip.timelineStartMs} onChange={e => controller.moveOverlayClipTransient(clip.id, Number(e.target.value))} /><span>{formatMs(clip.timelineStartMs)}</span></label>
      <label>时长 <input type="range" min={200} max={12000} step={100} value={dur} onChange={e => controller.updateOverlayClipDuration(clip.id, Number(e.target.value))} /><span>{dur}ms</span></label>
      <div className="inspector-actions">
        <button className="outline-button" onClick={() => controller.duplicateSelected()}>复制</button>
        <button className="outline-button" onClick={() => controller.deleteSelected()}>删除</button>
      </div>
      <p className="inspector-note">自由定位：拖动 body 定位，右柄改时长。</p>
    </div>
  )
}
function AudioInspector({ clip, controller }: { clip: Mash['tracks'][number]['clips'][number]; controller: StudioWorkspaceController }) {
  const vol = typeof clip.volume === 'number' ? clip.volume : 1
  const fi = clip.fadeInMs ?? 0
  const fo = clip.fadeOutMs ?? 0
  const loop = clip.loopEnabled ?? false
  const trackId = clip.trackId ?? ''
  const track = controller.mash?.tracks.find(t=>t.id===trackId)
  return (
    <div className="inspector-body" style={{display:'grid', gap:10}}>
      <div style={{display:'grid', gap:6, padding:8, background:'#252721', borderRadius:6, border:'1px solid #3a3c36'}}>
        <label style={{display:'flex', justifyContent:'space-between', alignItems:'center'}}>轨道启用 <input type="checkbox" checked={track?.enabled ?? true} onChange={()=>controller.toggleAudioTrackEnabled(trackId)} /></label>
        <small style={{opacity:0.6}}>禁用后该轨在预览混音中静音</small>
      </div>
      <label>音量 <input type="range" min={0} max={200} step={5} value={Math.round(vol*100)} onChange={e => controller.updateAudioVolume(clip.id, Number(e.target.value)/100)} /><span>{Math.round(vol*100)}%</span></label>
      <div style={{display:'grid', gridTemplateColumns:'1fr 1fr', gap:8}}>
        <label>淡入 ms <input type="number" value={fi} onChange={e=>controller.updateAudioFadeLoop(clip.id,{fadeInMs:Number(e.target.value)})} style={{width:'100%'}}/></label>
        <label>淡出 ms <input type="number" value={fo} onChange={e=>controller.updateAudioFadeLoop(clip.id,{fadeOutMs:Number(e.target.value)})} style={{width:'100%'}}/></label>
      </div>
      <label style={{display:'flex', gap:8, alignItems:'center'}}><input type="checkbox" checked={loop} onChange={e=>controller.updateAudioFadeLoop(clip.id,{loopEnabled:e.target.checked})}/> 循环铺满</label>
      <div className="inspector-actions">
        <button className="outline-button" onClick={() => controller.updateAudioVolume(clip.id, 1)}>重置 100%</button>
        <button className="outline-button" onClick={() => controller.updateAudioVolume(clip.id, 0)}>静音</button>
      </div>
      <label>时长 <input type="number" value={clip.timelineEndMs - clip.timelineStartMs} disabled style={{width:'100%'}}/></label>
    </div>
  )
}
function TextInspector({ clip, controller }: { clip: Mash['tracks'][number]['clips'][number]; controller: StudioWorkspaceController }) {
  const curTemplate = (clip.templateId ?? 'subtitle_safe') as string
  const s = clip.textStyle ?? { fontKey:'jianying_default', fontSize:0.055, bold:true, color:'#FFFFFF', strokeColor:null, strokeWidth:0, shadow:false, backgroundColor:null, alignment:'center', letterSpacing:0, lineSpacing:0 }
  const l = clip.textLayout ?? { anchor:'bottom', x:0.5, y:0.82, maxWidth:0.86, safeArea:'title_safe' }
  const ent = clip.textEntrance
  const ex = clip.textExit
  const lp = clip.textLoop
  const track = controller.mash?.tracks.find(t=>t.id===clip.trackId)
  return (
    <div className="inspector-body" style={{display:'grid', gap:10}}>
      <label>文案<textarea rows={3} value={clip.text ?? ''} onChange={e => controller.updateTextCue(clip.id, e.target.value)} maxLength={280} style={{width:'100%'}}/></label>
      <small>{(clip.text ?? '').length}/280</small>
      <div style={{display:'grid', gap:6, padding:8, background:'#252721', borderRadius:6, border:'1px solid #3a3c36'}}>
        <label style={{display:'flex', justifyContent:'space-between'}}>轨道启用 <input type="checkbox" checked={track?.enabled ?? true} onChange={()=>{ if(track) controller.updateTextTrackMeta(track.id,{enabled: !track.enabled})}} /></label>
        <label style={{display:'flex', justifyContent:'space-between'}}>锁定 <input type="checkbox" checked={!!track?.locked} onChange={()=>{ if(track) controller.updateTextTrackMeta(track.id,{locked: !track.locked})}} /></label>
        <label style={{display:'flex', justifyContent:'space-between'}}>可编辑 <input type="checkbox" checked={track?.editable ?? true} onChange={()=>{ if(track) controller.updateTextTrackMeta(track.id,{editable: !(track.editable ?? true)})}} /></label>
      </div>
      <label>预设
        <select value={curTemplate} onChange={e => { const tid=e.target.value; controller.updateTextTemplate(clip.id,tid); const p=SUBTITLE_PRESETS.find(x=>x.templateId===tid); if(p) controller.updateTextStyle(clip.id,p.style as never)}} style={{width:'100%'}}>
          {SUBTITLE_PRESETS.map(p => <option key={p.templateId} value={p.templateId}>{p.name} · {p.description}</option>)}
          <option value="subtitle_safe">经典描边</option>
        </select>
      </label>
      <div style={{display:'grid', gridTemplateColumns:'1fr 1fr', gap:8}}>
        <label>字号 {Math.round(s.fontSize*960)}px <input type="range" min={0.03} max={0.12} step={0.005} value={s.fontSize} onChange={e=>controller.updateTextStyle(clip.id,{fontSize: Number(e.target.value)})} style={{width:'100%'}}/></label>
        <label>字体<select value={s.fontKey} onChange={e=>controller.updateTextStyle(clip.id,{fontKey:e.target.value})} style={{width:'100%'}}><option value="jianying_default">默认</option><option value="sans">无衬线</option><option value="serif">衬线</option><option value="handwritten">手写</option></select></label>
        <label>字色 <input type="color" value={s.color} onChange={e=>controller.updateTextStyle(clip.id,{color:e.target.value})} style={{width:'100%'}}/></label>
        <label>描边色 <input type="color" value={s.strokeColor ?? '#000000'} onChange={e=>controller.updateTextStyle(clip.id,{strokeColor:e.target.value})} style={{width:'100%'}}/><button className="outline-button" style={{marginTop:4}} onClick={()=>controller.updateTextStyle(clip.id,{strokeColor: null})}>无描边</button></label>
        <label>描边宽 {s.strokeWidth} <input type="range" min={0} max={12} step={0.5} value={s.strokeWidth} onChange={e=>controller.updateTextStyle(clip.id,{strokeWidth:Number(e.target.value)})} style={{width:'100%'}}/></label>
        <label>阴影 <input type="checkbox" checked={s.shadow} onChange={e=>controller.updateTextStyle(clip.id,{shadow:e.target.checked})} /></label>
        <label>底色 <input type="color" value={s.backgroundColor ?? '#000000'} onChange={e=>controller.updateTextStyle(clip.id,{backgroundColor:e.target.value})} style={{width:'100%'}}/><button className="outline-button" style={{marginTop:4}} onClick={()=>controller.updateTextStyle(clip.id,{backgroundColor: null})}>无底色</button></label>
        <label>对齐<select value={s.alignment} onChange={e=>controller.updateTextStyle(clip.id,{alignment:e.target.value})} style={{width:'100%'}}><option value="left">左</option><option value="center">中</option><option value="right">右</option></select></label>
        <label>字距 {s.letterSpacing} <input type="range" min={-2} max={10} step={1} value={s.letterSpacing} onChange={e=>controller.updateTextStyle(clip.id,{letterSpacing:Number(e.target.value)})} style={{width:'100%'}}/></label>
        <label>行距 {s.lineSpacing} <input type="range" min={-10} max={20} step={1} value={s.lineSpacing} onChange={e=>controller.updateTextStyle(clip.id,{lineSpacing:Number(e.target.value)})} style={{width:'100%'}}/></label>
      </div>
      <div style={{display:'grid', gridTemplateColumns:'1fr 1fr', gap:8, padding:8, background:'#252721', borderRadius:6, border:'1px solid #3a3c36'}}>
        <label>锚点<select value={l.anchor} onChange={e=>controller.updateTextLayout(clip.id,{anchor:e.target.value})} style={{width:'100%'}}><option value="top">顶部</option><option value="middle_center">居中</option><option value="bottom">底部</option><option value="bottom_left">左下</option><option value="bottom_right">右下</option><option value="top_left">左上</option><option value="top_right">右上</option></select></label>
        <label>安全区<select value={l.safeArea} onChange={e=>controller.updateTextLayout(clip.id,{safeArea:e.target.value})} style={{width:'100%'}}><option value="title_safe">标题安全</option><option value="allow_bottom">允许底部</option><option value="default">默认</option></select></label>
        <label>X {l.x.toFixed(2)} <input type="range" min={0} max={1} step={0.02} value={l.x} onChange={e=>controller.updateTextLayout(clip.id,{x:Number(e.target.value)})} style={{width:'100%'}}/></label>
        <label>Y {l.y.toFixed(2)} <input type="range" min={0} max={1} step={0.02} value={l.y} onChange={e=>controller.updateTextLayout(clip.id,{y:Number(e.target.value)})} style={{width:'100%'}}/></label>
        <label>最大宽度 {l.maxWidth.toFixed(2)} <input type="range" min={0.5} max={1} step={0.02} value={l.maxWidth} onChange={e=>controller.updateTextLayout(clip.id,{maxWidth:Number(e.target.value)})} style={{width:'100%'}}/></label>
      </div>
      <div style={{display:'grid', gridTemplateColumns:'1fr 1fr', gap:8}}>
        <label>入场<select value={ent?.templateId ?? ''} onChange={e=>controller.updateTextAnimation(clip.id,'entrance', e.target.value ? { templateId:e.target.value, durationMs:220, intensity:1 } : null)} style={{width:'100%'}}><option value="">无</option><option value="fade">渐显</option><option value="wipe">擦除</option><option value="slide_up">上滑</option><option value="slide_down">下滑</option><option value="pop">弹入</option></select></label>
        <label>出场<select value={ex?.templateId ?? ''} onChange={e=>controller.updateTextAnimation(clip.id,'exit', e.target.value ? { templateId:e.target.value, durationMs:180, intensity:1 } : null)} style={{width:'100%'}}><option value="">无</option><option value="fade">渐隐</option><option value="wipe">擦除</option><option value="slide_up">上滑</option></select></label>
        <label>循环<select value={lp?.templateId ?? ''} onChange={e=>controller.updateTextAnimation(clip.id,'loop', e.target.value ? { templateId:e.target.value, durationMs:400, intensity:0.8 } : null)} style={{width:'100%'}}><option value="">无</option><option value="fade">呼吸</option><option value="pop">弹跳</option></select></label>
      </div>
      <div className="inspector-actions" style={{flexWrap:'wrap'}}>
        {SUBTITLE_PRESETS.slice(0,8).map(p => (
          <button key={p.presetId} className="outline-button" style={{padding:'4px 6px', fontSize:10}} title={p.description} onClick={() => { controller.updateTextTemplate(clip.id, p.templateId); controller.updateTextStyle(clip.id, p.style as never) }}>{p.name}</button>
        ))}
      </div>
    </div>
  )
}

function waveformHeights(clipId: string, width: number): number[] {
  let hash = 0
  for (let i = 0; i < clipId.length; i += 1) hash = ((hash << 5) - hash + clipId.charCodeAt(i)) | 0
  const count = Math.max(6, Math.min(24, Math.floor(width / 8) || 6))
  const out: number[] = []
  for (let i = 0; i < count; i += 1) {
    const r = Math.abs(Math.sin(hash * 0.13 + i * 1.37) % 1)
    out.push(18 + Math.round(r * 62))
  }
  return out
}
function rulerTicks(durationMs: number, tickInterval: number, labelInterval: number, pps: number) {
  const ticks: Array<{ s: number; label: boolean }> = []
  const durS = durationMs / 1000
  const step = tickInterval
  const count = Math.ceil(durS / step) + 1
  const maxTicks = 800
  const stride = count > maxTicks ? Math.ceil(count / maxTicks) : 1
  for (let i = 0; i < count; i += stride) {
    const s = i * step
    if (s > durS + 0.001) break
    const isLabel = Math.abs((s % labelInterval)) < 1e-4 || Math.abs((s % labelInterval) - labelInterval) < 1e-4
    void pps
    ticks.push({ s, label: isLabel })
  }
  return ticks
}
