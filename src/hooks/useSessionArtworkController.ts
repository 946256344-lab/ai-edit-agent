// 会话封面：从已保存镜头的真实关键帧读取，不依赖文件名和当前素材分页。
import { useEffect, useState } from 'react'
import { convertFileSrc } from '@tauri-apps/api/core'
import { getLatestStoryboard, getAssetEvidence } from '../lib/local-store'
import type { EditingSessionView } from '../components/workspace-types'
import { messages } from '../lib/i18n'

export function useSessionArtworkController(projectId: string | null, sessions: EditingSessionView[]) {
  const sessionIds = sessions.map((session) => session.id).join(',')
  const [artwork, setArtwork] = useState<{ projectId: string; covers: Record<string, string>; notice: string | null } | null>(null)
  useEffect(() => {
    if (!projectId || !sessionIds) return
    let active = true
    void Promise.all(sessionIds.split(',').map(async (sessionId) => {
      const storyboard = await getLatestStoryboard(projectId, sessionId)
      const shot = storyboard?.shots[0]
      if (!shot) return [sessionId, ''] as const
      const evidence = await getAssetEvidence(shot.assetId)
      const frame = evidence.keyframes.find((item) => item.timeMs >= shot.sourceStartMs && item.timeMs < shot.sourceEndMs)
      return [sessionId, frame ? convertFileSrc(frame.imagePath) : ''] as const
    })).then((entries) => {
      if (active) setArtwork({ projectId, covers: Object.fromEntries(entries), notice: null })
    }).catch(() => {
      if (active) setArtwork({ projectId, covers: {}, notice: messages().app.coversFailed })
    })
    return () => { active = false }
  }, [projectId, sessionIds])
  return artwork?.projectId === projectId ? artwork : null
}
