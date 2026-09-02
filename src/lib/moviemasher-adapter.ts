// MovieMasher 适配器：内部 TimelineVersion 与 mash 结构的双向映射。
// mash 是前端工作台的中间表示，始终以 Rust 侧 TimelineVersion 为事实源。

import type { TimelineVersion, TextTrack, TextCue } from './local-store'

// 前端工作台使用的中间表示，对齐 moviemasher.js 的 tracks/clips 概念。
export type MashTrackKind = 'video' | 'text' | 'audio' | 'voiceover' | 'overlay'

export type MashClip = {
  id: string
  kind: MashTrackKind
  assetId: string | null
  text: string | null
  timelineStartMs: number
  timelineEndMs: number
  sourceStartMs: number
  sourceEndMs: number
  trackId: string | null
  role: string | null
  volume?: number
  loopEnabled?: boolean
  fadeInMs?: number
  fadeOutMs?: number
  templateId?: string | null
  textStyle?: TextCue['style'] | null
  textLayout?: TextCue['layout'] | null
  textEntrance?: TextCue['entrance'] | null
  textExit?: TextCue['exit'] | null
  textLoop?: TextCue['loopAnimation'] | null
}

export type MashTrack = {
  id: string
  kind: MashTrackKind
  label: string
  enabled: boolean
  editable?: boolean
  locked?: boolean
  origin?: string
  clips: MashClip[]
}

export type Mash = {
  id: string
  durationMs: number
  tracks: MashTrack[]
}

export type MashInsertedClip = {
  assetId: string
  sourceStartMs: number
  sourceEndMs: number
  timelineStartMs: number
  timelineEndMs: number
  onScreenText: string
  derivedFromShotIndex: number | null
}

export type MashDiff = {
  hasChanges: boolean
  orderChanged: boolean
  durationsChanged: boolean
  textChanged: boolean
  inserted: MashInsertedClip[]
  deletedShotIndices: number[]
  newOrder: number[] | null
  durationAdjustments: Array<{ shotIndex: number; newDurationMs: number; newSourceStartMs: number }>
  clipReplacements: Array<{ shotIndex: number; assetId: string; sourceStartMs: number; sourceEndMs: number }>
  updatedTextTracks: TextTrack[] | null
  overlayInserted: MashInsertedClip[]
  overlayDeletedShotIndices: number[]
  overlayOrderChanged: boolean
  overlayNewOrder: number[] | null
  overlayDurationAdjustments: Array<{ shotIndex: number; newDurationMs: number; newSourceStartMs: number; newTimelineStartMs: number }>
  overlayChanged: boolean
  audioChanged: boolean
  audioVolumeAdjustments: Array<{ cueId: string; trackId: string; newVolume: number }>
  updatedMusicTracks: import('./local-store').MusicTrackState[] | null
  updatedVoiceoverTracks: import('./local-store').VoiceoverTrackState[] | null
}

function parseKeptShotIndex(id: string): number | null {
  const m = id.match(/^clip-(\d+)$/)
  return m ? Number.parseInt(m[1], 10) : null
}

function parseDerivedFromShotIndex(id: string): number | null {
  const m = id.match(/^clip-(\d+)_/)
  return m ? Number.parseInt(m[1], 10) : null
}

export function timelineToMash(timeline: TimelineVersion): Mash {
  const tracks: MashTrack[] = []

  if (timeline.clips.length > 0) {
    tracks.push({
      id: 'video-main',
      kind: 'video',
      label: '视频',
      enabled: true,
      clips: timeline.clips.map((clip) => ({
        id: `clip-${clip.shotIndex}`,
        kind: 'video',
        assetId: clip.assetId,
        text: clip.onScreenText || null,
        timelineStartMs: clip.timelineStartMs,
        timelineEndMs: clip.timelineEndMs,
        sourceStartMs: clip.sourceStartMs,
        sourceEndMs: clip.sourceEndMs,
        trackId: 'video-main',
        role: null,
      })),
    })
  }

  for (const track of timeline.textTracks) {
    tracks.push({
      id: track.id,
      kind: 'text',
      label: textRoleLabel(track.role),
      enabled: track.enabled,
      editable: track.editable ?? true,
      locked: track.locked ?? false,
      origin: track.origin ?? 'storyboard_generated',
      clips: track.cues.map((cue) => ({
        id: cue.id,
        kind: 'text',
        assetId: null,
        text: cue.text,
        timelineStartMs: cue.startMs,
        timelineEndMs: cue.endMs,
        sourceStartMs: cue.startMs,
        sourceEndMs: cue.endMs,
        trackId: track.id,
        role: track.role,
        templateId: cue.templateId ?? null,
        textStyle: cue.style,
        textLayout: cue.layout,
        textEntrance: cue.entrance,
        textExit: cue.exit,
        textLoop: cue.loopAnimation ?? null,
      })),
    })
  }

  for (const mt of timeline.musicTracks ?? []) {
    if (!mt.cues || mt.cues.length === 0) continue
    tracks.push({
      id: mt.id,
      kind: 'audio',
      label: '音乐',
      enabled: mt.enabled,
      clips: mt.cues.map((cue) => ({
        id: cue.id,
        kind: 'audio',
        assetId: cue.assetId,
        text: null,
        timelineStartMs: cue.timelineStartMs,
        timelineEndMs: cue.timelineEndMs,
        sourceStartMs: cue.sourceStartMs,
        sourceEndMs: cue.sourceEndMs,
        trackId: mt.id,
        role: 'music',
        volume: cue.volume,
        loopEnabled: cue.loopEnabled ?? false,
        fadeInMs: cue.fadeInMs ?? 0,
        fadeOutMs: cue.fadeOutMs ?? 0,
      })),
    })
  }

  for (const vt of timeline.voiceoverTracks ?? []) {
    if (!vt.cues || vt.cues.length === 0) continue
    tracks.push({
      id: vt.id,
      kind: 'audio',
      label: '旁白',
      enabled: vt.enabled,
      clips: vt.cues.map((cue) => ({
        id: cue.id,
        kind: 'audio',
        assetId: cue.assetId,
        text: (cue as { voiceName?: string | null }).voiceName ?? null,
        timelineStartMs: cue.timelineStartMs,
        timelineEndMs: cue.timelineEndMs,
        sourceStartMs: cue.sourceStartMs,
        sourceEndMs: cue.sourceEndMs,
        trackId: vt.id,
        role: 'voiceover',
        volume: cue.volume,
        loopEnabled: false,
        fadeInMs: cue.fadeInMs ?? 0,
        fadeOutMs: cue.fadeOutMs ?? 0,
      })),
    })
  }

  if ((timeline.overlayClips ?? []).length > 0) {
    tracks.push({
      id: 'overlay-main',
      kind: 'overlay',
      label: '叠加',
      enabled: true,
      clips: (timeline.overlayClips ?? []).map((clip) => ({
        id: `overlay-${clip.shotIndex}`,
        kind: 'overlay',
        assetId: clip.assetId,
        text: clip.onScreenText || null,
        timelineStartMs: clip.timelineStartMs,
        timelineEndMs: clip.timelineEndMs,
        sourceStartMs: clip.sourceStartMs,
        sourceEndMs: clip.sourceEndMs,
        trackId: 'overlay-main',
        role: null,
      })),
    })
  }

  const durationMs = tracks.reduce((max, t) => {
    for (const c of t.clips) max = Math.max(max, c.timelineEndMs)
    return max
  }, 0)

  return { id: timeline.id, durationMs, tracks }
}

export function mashToDiff(original: TimelineVersion, editedMash: Mash): MashDiff {
  const videoTrack = editedMash.tracks.find((track) => track.kind === 'video')
  const editedClips = videoTrack?.clips ?? []

  const inserted: MashInsertedClip[] = editedClips
    .filter((clip) => parseKeptShotIndex(clip.id) === null)
    .map((clip) => ({
      assetId: clip.assetId ?? '',
      sourceStartMs: clip.sourceStartMs,
      sourceEndMs: clip.sourceEndMs,
      timelineStartMs: clip.timelineStartMs,
      timelineEndMs: clip.timelineEndMs,
      onScreenText: clip.text ?? '',
      derivedFromShotIndex: parseDerivedFromShotIndex(clip.id),
    }))

  const keptClips = editedClips.filter((clip) => parseKeptShotIndex(clip.id) !== null)
  const keptOrder = keptClips.map((clip) => parseKeptShotIndex(clip.id)!)
  const originalByShot = new Map(original.clips.map((clip) => [clip.shotIndex, clip]))
  const originalOrder = original.clips.map((clip) => clip.shotIndex)
  const deletedShotIndices = original.clips
    .filter((clip) => !keptOrder.includes(clip.shotIndex))
    .map((clip) => clip.shotIndex)

  const originalFilteredOrder = originalOrder.filter((s) => !deletedShotIndices.includes(s))
  const orderChanged = keptOrder.length === originalFilteredOrder.length
    && keptOrder.some((shot, index) => shot !== originalFilteredOrder[index])

  const clipReplacements: Array<{ shotIndex: number; assetId: string; sourceStartMs: number; sourceEndMs: number }> = []
  const durationAdjustments: Array<{ shotIndex: number; newDurationMs: number; newSourceStartMs: number }> = []
  let durationsChanged = false
  for (const clip of keptClips) {
    const shotIndex = parseKeptShotIndex(clip.id)!
    const orig = originalByShot.get(shotIndex)
    if (!orig) { durationsChanged = true; continue }
    const assetChanged = clip.assetId !== null && clip.assetId !== orig.assetId
    if (assetChanged) {
      clipReplacements.push({ shotIndex, assetId: clip.assetId!, sourceStartMs: clip.sourceStartMs, sourceEndMs: clip.sourceEndMs })
      durationsChanged = true
      continue
    }
    const origDuration = orig.timelineEndMs - orig.timelineStartMs
    const newDuration = clip.timelineEndMs - clip.timelineStartMs
    const sourceChanged = orig.sourceStartMs !== clip.sourceStartMs || origDuration !== newDuration
    if (sourceChanged) {
      durationsChanged = true
      durationAdjustments.push({ shotIndex, newDurationMs: newDuration, newSourceStartMs: clip.sourceStartMs })
    }
  }

  let textChanged = false
  let updatedTextTracks: TextTrack[] | null = null
  const editedTextTracks = editedMash.tracks.filter((t) => t.kind === 'text')
  const trackMetaChanged = editedTextTracks.some((t) => {
    const o = original.textTracks.find((x) => x.id === t.id)
    if (!o) return true
    return o.enabled !== t.enabled || (o.editable ?? true) !== (t.editable ?? true) || (o.locked ?? false) !== (t.locked ?? false)
  })
  if (editedTextTracks.length !== original.textTracks.length || trackMetaChanged) {
    textChanged = true
  } else {
    for (const track of editedTextTracks) {
      const origTrack = original.textTracks.find((t) => t.id === track.id)
      if (!origTrack) { textChanged = true; break }
      if (track.clips.length !== origTrack.cues.length) { textChanged = true; break }
      for (let i = 0; i < track.clips.length; i += 1) {
        const ec = track.clips[i]
        const oc = origTrack.cues[i]
        if (ec.text !== oc.text
          || ec.timelineStartMs !== oc.startMs
          || ec.timelineEndMs !== oc.endMs
          || (ec.templateId ?? null) !== (oc.templateId ?? null)
          || JSON.stringify(ec.textStyle ?? null) !== JSON.stringify(oc.style ?? null)
          || JSON.stringify(ec.textLayout ?? null) !== JSON.stringify(oc.layout ?? null)
          || JSON.stringify(ec.textEntrance ?? null) !== JSON.stringify(oc.entrance ?? null)
          || JSON.stringify(ec.textExit ?? null) !== JSON.stringify(oc.exit ?? null)
          || JSON.stringify(ec.textLoop ?? null) !== JSON.stringify(oc.loopAnimation ?? null)) {
          textChanged = true; break
        }
      }
      if (textChanged) break
    }
  }
  if (textChanged) {
    updatedTextTracks = editedTextTracks.map((track) => {
      const origTrack = original.textTracks.find((t) => t.id === track.id)
      return {
        id: track.id,
        role: (origTrack?.role ?? 'subtitle') as TextTrack['role'],
        layer: origTrack?.layer ?? 1,
        enabled: track.enabled,
        editable: track.editable ?? true,
        locked: track.locked ?? false,
        origin: track.origin as TextTrack['origin'] ?? origTrack?.origin ?? 'storyboard_generated',
        generationId: origTrack?.generationId ?? null,
        cues: track.clips.map((clip): TextCue => {
          const oc = origTrack?.cues.find((c) => c.id === clip.id)
          return {
            id: clip.id,
            templateId: (clip.templateId ?? oc?.templateId ?? null) as string | null,
            startMs: clip.timelineStartMs,
            endMs: clip.timelineEndMs,
            text: clip.text ?? '',
            style: (clip.textStyle ?? oc?.style ?? defaultTextStyle()) as TextCue['style'],
            layout: (clip.textLayout ?? oc?.layout ?? defaultTextLayout()) as TextCue['layout'],
            entrance: (clip.textEntrance ?? oc?.entrance ?? null) as TextCue['entrance'],
            exit: (clip.textExit ?? oc?.exit ?? null) as TextCue['exit'],
            loopAnimation: (clip.textLoop ?? oc?.loopAnimation ?? null) as TextCue['loopAnimation'],
            jianyingCompatibility: 'verified',
          }
        }),
      } as unknown as TextTrack
    })
  }

  // overlay diff
  const overlayTrack = editedMash.tracks.find((t) => t.kind === 'overlay')
  const overlayClips = overlayTrack?.clips ?? []
  const origOverlay = original.overlayClips ?? []
  const origOverlayByShot = new Map(origOverlay.map((c) => [c.shotIndex, c]))
  const overlayInserted: MashInsertedClip[] = overlayClips
    .filter((c) => c.id.startsWith('overlay-new-') || !origOverlayByShot.has(Number.parseInt(c.id.replace('overlay-', ''), 10)))
    .map((c) => ({
      assetId: c.assetId ?? '',
      sourceStartMs: c.sourceStartMs,
      sourceEndMs: c.sourceEndMs,
      timelineStartMs: c.timelineStartMs,
      timelineEndMs: c.timelineEndMs,
      onScreenText: c.text ?? '',
      derivedFromShotIndex: null,
    }))
  const overlayKept = overlayClips.filter((c) => !c.id.startsWith('overlay-new-') && origOverlayByShot.has(Number.parseInt(c.id.replace('overlay-', ''), 10)))
  const overlayDeletedShotIndices = origOverlay.filter((c) => !overlayKept.some((k) => Number.parseInt(k.id.replace('overlay-', ''), 10) === c.shotIndex)).map((c) => c.shotIndex)
  const overlayKeptOrder = overlayKept.map((c) => Number.parseInt(c.id.replace('overlay-', ''), 10))
  const origOverlayOrder = origOverlay.map((c) => c.shotIndex)
  const origOverlayFiltered = origOverlayOrder.filter((s) => !overlayDeletedShotIndices.includes(s))
  const overlayOrderChanged = overlayKeptOrder.length === origOverlayFiltered.length && overlayKeptOrder.some((s, i) => s !== origOverlayFiltered[i])
  const overlayDurationAdjustments = overlayKept
    .filter((c) => {
      const shotIndex = Number.parseInt(c.id.replace('overlay-', ''), 10)
      const orig = origOverlayByShot.get(shotIndex)
      if (!orig) return false
      return orig.timelineStartMs !== c.timelineStartMs || (orig.timelineEndMs - orig.timelineStartMs) !== (c.timelineEndMs - c.timelineStartMs) || orig.sourceStartMs !== c.sourceStartMs
    })
    .map((c) => ({
      shotIndex: Number.parseInt(c.id.replace('overlay-', ''), 10),
      newDurationMs: c.timelineEndMs - c.timelineStartMs,
      newSourceStartMs: c.sourceStartMs,
      newTimelineStartMs: c.timelineStartMs,
    }))
  const overlayChanged = overlayInserted.length > 0 || overlayDeletedShotIndices.length > 0 || overlayOrderChanged || overlayDurationAdjustments.length > 0

  // audio diff: volume/fade/loop/enabled
  const audioTracks = editedMash.tracks.filter((t) => t.kind === 'audio')
  let audioChanged = false
  const audioVolumeAdjustments: Array<{ cueId: string; trackId: string; newVolume: number }> = []
  const updatedMusicTracks: import('./local-store').MusicTrackState[] = []
  const updatedVoiceoverTracks: import('./local-store').VoiceoverTrackState[] = []
  for (const track of audioTracks) {
    const isMusic = track.label === '音乐'
    const origList = isMusic ? (original.musicTracks ?? []) : (original.voiceoverTracks ?? [])
    const origTrack = origList.find((t) => t.id === track.id)
    if (!origTrack) continue
    const origById = new Map(origTrack.cues.map((c) => [c.id, c as unknown as { volume: number; fadeInMs?: number; fadeOutMs?: number; loopEnabled?: boolean }]))
    const newCues: unknown[] = []
    let trackChanged = false
    for (const c of track.clips) {
      const oc = origById.get(c.id) as unknown as { volume: number; fadeInMs?: number; fadeOutMs?: number; loopEnabled?: boolean } | undefined
      const vol = typeof c.volume === 'number' ? Math.max(0, Math.min(2, c.volume)) : (oc?.volume ?? 1)
      const fi = typeof c.fadeInMs === 'number' ? Math.max(0, c.fadeInMs) : (oc?.fadeInMs ?? 0)
      const fo = typeof c.fadeOutMs === 'number' ? Math.max(0, c.fadeOutMs) : (oc?.fadeOutMs ?? 0)
      const lp = typeof c.loopEnabled === 'boolean' ? c.loopEnabled : (oc?.loopEnabled ?? false)
      if (oc && (Math.abs((oc.volume ?? 1) - vol) > 0.01 || (oc.fadeInMs ?? 0) !== fi || (oc.fadeOutMs ?? 0) !== fo || (oc.loopEnabled ?? false) !== lp)) {
        trackChanged = true
      }
      if (oc && Math.abs((oc.volume ?? 1) - vol) > 0.01) audioVolumeAdjustments.push({ cueId: c.id, trackId: track.id, newVolume: vol })
      newCues.push({ id: c.id, assetId: c.assetId ?? '', sourceStartMs: c.sourceStartMs, sourceEndMs: c.sourceEndMs, timelineStartMs: c.timelineStartMs, timelineEndMs: c.timelineEndMs, volume: vol, fadeInMs: fi, fadeOutMs: fo, loopEnabled: lp })
    }
    if (trackChanged || (origTrack.enabled !== track.enabled)) audioChanged = true
    if (isMusic) updatedMusicTracks.push({ id: track.id, enabled: track.enabled, cues: newCues as unknown as import('./local-store').MusicCue[] })
    else {
      const ov = origTrack as unknown as { cues: Array<{ voiceName?: string | null }> }
      updatedVoiceoverTracks.push({ id: track.id, enabled: track.enabled, cues: newCues.map((x, i) => ({ ...(x as object), voiceName: (ov.cues[i] as { voiceName?: string })?.voiceName ?? null })) as unknown as import('./local-store').VoiceoverCue[] })
    }
  }
  for (const t of audioTracks) {
    const list = t.label === '音乐' ? (original.musicTracks ?? []) : (original.voiceoverTracks ?? [])
    const o = list.find((x) => x.id === t.id)
    if (o && o.enabled !== t.enabled) { audioChanged = true; break }
  }

  const hasChanges = inserted.length > 0 || deletedShotIndices.length > 0 || orderChanged || durationsChanged || textChanged || overlayChanged || audioChanged || clipReplacements.length > 0

  return {
    hasChanges,
    orderChanged,
    durationsChanged,
    textChanged,
    inserted,
    deletedShotIndices,
    newOrder: orderChanged ? keptOrder : null,
    durationAdjustments: durationsChanged ? durationAdjustments : [],
    clipReplacements,
    updatedTextTracks,
    overlayInserted,
    overlayDeletedShotIndices,
    overlayOrderChanged,
    overlayNewOrder: overlayOrderChanged ? overlayKeptOrder : null,
    overlayDurationAdjustments,
    overlayChanged,
    audioChanged,
    audioVolumeAdjustments,
    updatedMusicTracks: audioChanged ? updatedMusicTracks as import('./local-store').MusicTrackState[] : null,
    updatedVoiceoverTracks: audioChanged ? updatedVoiceoverTracks as import('./local-store').VoiceoverTrackState[] : null,
  }
}

export function formatMs(ms: number): string {
  const totalSec = Math.floor(ms / 1000)
  const m = Math.floor(totalSec / 60)
  const s = totalSec % 60
  const rem = ms % 1000
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}.${String(Math.floor(rem / 100)).padStart(1, '0')}`
}

function textRoleLabel(role: string): string {
  switch (role) {
    case 'subtitle': return '字幕'
    case 'headline': return '标题'
    case 'callout': return '标注'
    case 'cta': return 'CTA'
    default: return role
  }
}

function defaultTextStyle(): TextCue['style'] {
  return {
    fontKey: 'jianying_default',
    fontSize: 0.055,
    bold: true,
    color: '#FFFFFF',
    strokeColor: null,
    strokeWidth: 0,
    shadow: false,
    backgroundColor: null,
    alignment: 'center',
    letterSpacing: 0,
    lineSpacing: 0,
  }
}

function defaultTextLayout(): TextCue['layout'] {
  return { anchor: 'bottom', x: 0.5, y: 0.82, maxWidth: 0.86, safeArea: 'title_safe' }
}
