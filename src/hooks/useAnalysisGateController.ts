// 剪辑前等待首次素材分析；只保存当前页面中的请求，取消或切换会话时释放等待。
import { useEffect, useRef, useState } from 'react'
import { getAssetAnalysisProgress } from '../lib/local-store'
import { analysisPendingCount } from '../lib/asset-analysis'
import type { AssetAnalysisProgress } from '../lib/local-store'

type PendingAnalysis = {
  projectId: string
  resolve: (proceed: boolean) => void
  reject: (error: unknown) => void
  timer?: number
}

export function useAnalysisGateController(projectId: string | null, sessionId: string | null, importing: boolean) {
  const pending = useRef<PendingAnalysis | null>(null)
  const importingRef = useRef(importing)
  importingRef.current = importing
  const [progress, setProgress] = useState<AssetAnalysisProgress | null>(null)
  const [waiting, setWaiting] = useState(false)

  function finish(proceed: boolean) {
    const request = pending.current
    if (!request) return
    pending.current = null
    window.clearTimeout(request.timer)
    setProgress(null)
    setWaiting(false)
    request.resolve(proceed)
  }

  async function check(request: PendingAnalysis, useReady = false) {
    try {
      const next = await getAssetAnalysisProgress(request.projectId)
      if (pending.current !== request) return
      setProgress(next)
      if (!importingRef.current && analysisPendingCount(next) === 0 && (next.failed + next.cancelled === 0 || (useReady && next.readyVideo > 0))) {
        finish(true)
        return
      }
      window.clearTimeout(request.timer)
      request.timer = window.setTimeout(() => void check(request), 1500)
    } catch (error) {
      if (pending.current !== request) return
      pending.current = null
      window.clearTimeout(request.timer)
      setProgress(null)
      setWaiting(false)
      request.reject(error)
    }
  }

  function waitForAnalysis() {
    if (!projectId) return Promise.resolve(true)
    return new Promise<boolean>((resolve, reject) => {
      const request = { projectId, resolve, reject }
      pending.current = request
      setWaiting(true)
      void check(request)
    })
  }

  useEffect(() => () => {
    const request = pending.current
    if (request) {
      pending.current = null
      window.clearTimeout(request.timer)
      request.resolve(false)
    }
    setProgress(null)
    setWaiting(false)
  }, [projectId, sessionId])

  return {
    progress,
    waiting,
    waitForAnalysis,
    cancel: () => finish(false),
    useReady: () => { if (pending.current) void check(pending.current, true) },
  }
}
