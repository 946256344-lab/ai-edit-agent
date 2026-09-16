// 对话媒体选项：会话内保留草稿选择，已发送的选择从本地任务记录恢复。
import { useState } from 'react'
import type { MediaOptions, StoredAgentTask } from '../lib/local-store'

const defaultMediaOptions: MediaOptions = { voiceover: true, subtitles: true, bgm: true }

export function useComposerMediaController(sessionId: string | null, tasks: StoredAgentTask[]) {
  const [drafts, setDrafts] = useState<Record<string, MediaOptions>>({})
  const key = sessionId ?? 'new'
  const saved = tasks.find((task) => task.editingTaskId === sessionId && task.input.mediaOptions)?.input.mediaOptions
  const options = drafts[key] ?? saved ?? defaultMediaOptions
  return {
    options,
    toggle: (name: keyof MediaOptions) => setDrafts((current) => ({
      ...current, [key]: { ...options, [name]: !options[name] },
    })),
    rememberSent: (targetSessionId: string, sent: MediaOptions) => setDrafts((current) => {
      const next = { ...current, [targetSessionId]: current[key] ?? sent }
      if (!sessionId) delete next.new
      return next
    }),
  }
}
