// 对话媒体选项：会话内保留草稿选择，已发送的选择从本地任务记录恢复。字幕跟随配音，画幅单独选择。
import { useState } from 'react'
import type { AspectRatio, MediaOptions, StoredAgentTask } from '../lib/local-store'

const defaultMediaOptions: MediaOptions = { voiceover: true, subtitles: true, bgm: true, aspectRatio: '9:16' }

export type MediaToggle = 'voiceover' | 'bgm'

export function useComposerMediaController(sessionId: string | null, tasks: StoredAgentTask[]) {
  const [drafts, setDrafts] = useState<Record<string, MediaOptions>>({})
  const key = sessionId ?? 'new'
  const saved = tasks.find((task) => task.editingTaskId === sessionId && task.input.mediaOptions)?.input.mediaOptions
  const current = drafts[key] ?? saved ?? defaultMediaOptions
  const options: MediaOptions = { ...current, subtitles: current.voiceover, aspectRatio: current.aspectRatio ?? '9:16' }
  const update = (next: MediaOptions) => setDrafts((all) => ({ ...all, [key]: { ...next, subtitles: next.voiceover } }))
  return {
    options,
    toggle: (name: MediaToggle) => update({ ...options, [name]: !options[name] }),
    setAspectRatio: (aspectRatio: AspectRatio) => update({ ...options, aspectRatio }),
    rememberSent: (targetSessionId: string, sent: MediaOptions) => setDrafts((all) => {
      const next = { ...all, [targetSessionId]: all[key] ?? sent }
      if (!sessionId) delete next.new
      return next
    }),
  }
}
