import type { TextTrack } from './local-store'

function assColor(color: string): string {
  const hex = color.trim().replace('#', '')
  if (hex.length !== 6 && hex.length !== 8) return '&H00FFFFFF'
  const rgb = hex.length === 8 ? hex.slice(2) : hex
  return `&H00${rgb.slice(4, 6)}${rgb.slice(2, 4)}${rgb.slice(0, 2)}`
}
function esc(text: string): string {
  return text.replace(/\\/g, '\\\\').replace(/{/g, '\\{').replace(/}/g, '\\}').replace(/\r?\n/g, '\\N')
}
function ts(ms: number): string {
  const cs = Math.max(0, Math.floor((ms + 5) / 10))
  return `${Math.floor(cs / 360000)}:${String(Math.floor((cs / 6000) % 60)).padStart(2,'0')}:${String(Math.floor((cs / 100) % 60)).padStart(2,'0')}.${String(cs % 100).padStart(2,'0')}`
}
function fontName(key: string): string {
  switch (key) {
    case 'jianying_handwritten': return 'KaiTi'
    case 'mono_tech': return 'Consolas'
    case 'serif_editorial': case 'jianying_serif_bold': return 'Georgia'
    default: return 'Microsoft YaHei'
  }
}

export function textTracksToAss(tracks: TextTrack[]): string {
  let styles = ''
  let events = ''
  let idx = 0
  for (const track of tracks.filter(t => t.enabled)) {
    for (const cue of track.cues) {
      const name = `s${idx++}`
      const size = Math.round(Math.max(12, cue.style.fontSize * 960))
      const outline = cue.style.strokeWidth
      const shadow = cue.style.shadow ? 2 : 0
      const align = cue.layout.anchor === 'top' ? 8 : cue.layout.anchor === 'center' ? 5 : 2
      styles += `Style: ${name},${fontName(cue.style.fontKey)},${size},${assColor(cue.style.color)},${assColor(cue.style.color)},${assColor(cue.style.strokeColor ?? '#000000')},&H00000000,${cue.style.bold ? -1 : 0},0,0,0,100,100,0,0,0,${align},0,0,${outline.toFixed(1)},${shadow},0,0,0,1\n`
      const x = Math.round(cue.layout.x * 540)
      const y = Math.round(cue.layout.y * 960)
      const tag = `{\\an${align}\\pos(${x},${y})}`
      events += `Dialogue: ${track.layer},${ts(cue.startMs)},${ts(cue.endMs)},${name},,0,0,0,,${tag}${esc(cue.text)}\n`
    }
  }
  return `[Script Info]\nScriptType: v4.00+\nPlayResX: 540\nPlayResY: 960\n\n[V4+ Styles]\nFormat: Name,Fontname,Fontsize,PrimaryColour,SecondaryColour,OutlineColour,BackColour,Bold,Italic,Underline,StrikeOut,ScaleX,ScaleY,Spacing,Angle,BorderStyle,Alignment,MarginL,MarginR,MarginV,Outline,Shadow,Encoding\n${styles}\n[Events]\nFormat: Layer,Start,End,Style,Name,MarginL,MarginR,MarginV,Effect,Text\n${events}`
}

export type JassubHandle = { destroy: () => void }

export async function attachJassub(
  video: HTMLVideoElement,
  container: HTMLElement,
  tracks: TextTrack[],
): Promise<JassubHandle | null> {
  if (!tracks || tracks.length === 0) return null
  const ass = textTracksToAss(tracks)
  try {
    const modWrap = await import('jassub')
    const JASSUB = (modWrap as unknown as { default?: unknown }).default ?? (modWrap as unknown)
    const Cls = JASSUB as unknown as new (opts: unknown) => { destroy?: () => void }
    const inst = new Cls({
      video,
      subContent: ass,
      container,
    } as never)
    return { destroy: () => inst.destroy?.() }
  } catch {
    return null
  }
}
