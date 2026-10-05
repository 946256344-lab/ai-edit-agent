// 对话媒体选项：会话内保留草稿选择，已发送的选择从本地任务记录恢复。字幕跟随配音，画幅单独选择。
// 网关明确没有配音能力时隐藏配音开关并按关闭发送，不换别的配音服务。
import { useEffect, useState } from 'react'
import { getVoiceAvailability } from '../lib/local-store'
import type { AspectRatio, GenreSelection, MediaOptions, StoredAgentTask } from '../lib/local-store'

const defaultMediaOptions: MediaOptions = { voiceover: true, subtitles: true, bgm: true, aspectRatio: '9:16', genre: 'auto' }
// 回到窗口时重新探测，但每次探测都要走一遍网关账号校验，间隔不低于 5 分钟。
const VOICE_PROBE_INTERVAL_MS = 5 * 60 * 1000

export type MediaToggle = 'voiceover' | 'bgm'

function useVoiceAvailability(desktopRuntime: boolean) {
  const [available, setAvailable] = useState(true)
  useEffect(() => {
    if (!desktopRuntime) return
    let active = true
    let lastProbe = 0
    const probe = () => {
      if (Date.now() - lastProbe < VOICE_PROBE_INTERVAL_MS) return
      lastProbe = Date.now()
      void getVoiceAvailability().then((result) => { if (active) setAvailable(result.available) }).catch(() => {})
    }
    probe()
    window.addEventListener('focus', probe)
    return () => { active = false; window.removeEventListener('focus', probe) }
  }, [desktopRuntime])
  return available
}

export function useComposerMediaController(sessionId: string | null, tasks: StoredAgentTask[], desktopRuntime: boolean) {
  const voiceAvailable = useVoiceAvailability(desktopRuntime)
  const [drafts, setDrafts] = useState<Record<string, MediaOptions>>({})
  const key = sessionId ?? 'new'
  const saved = tasks.filter((task) => task.editingTaskId === sessionId && task.input.mediaOptions)
    .sort((a, b) => b.createdAt - a.createdAt)[0]?.input.mediaOptions
  const current = drafts[key] ?? saved ?? defaultMediaOptions
  // 草稿保留用户原本的配音选择，配音恢复可用时照旧生效。
  const voiceover = current.voiceover && voiceAvailable
  const options: MediaOptions = { ...current, voiceover, subtitles: voiceover, aspectRatio: current.aspectRatio ?? '9:16', genre: current.genre ?? 'auto' }
  const update = (next: MediaOptions) => setDrafts((all) => ({ ...all, [key]: { ...next, subtitles: next.voiceover } }))
  return {
    options,
    voiceAvailable,
    toggle: (name: MediaToggle) => {
      if (name === 'voiceover' && !voiceAvailable) return
      update({ ...current, [name]: !options[name] })
    },
    setAspectRatio: (aspectRatio: AspectRatio) => update({ ...current, aspectRatio }),
    setGenre: (genre: GenreSelection) => update({ ...current, genre }),
    rememberSent: (targetSessionId: string, sent: MediaOptions) => setDrafts((all) => {
      const next = { ...all, [targetSessionId]: all[key] ?? sent }
      if (!sessionId) delete next.new
      return next
    }),
  }
}
