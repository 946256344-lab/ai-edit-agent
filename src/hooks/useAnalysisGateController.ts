// 剪辑前等待首次素材分析；未完成时弹出确认，取消或切换会话时释放等待。
import { useEffect, useRef, useState } from 'react'
import { getAssetAnalysisProgress } from '../lib/local-store'
import { analysisPendingCount } from '../lib/asset-analysis'
import type { AssetAnalysisProgress } from '../lib/local-store'

type PendingAnalysis = {
  projectId: string
  resolve: (proceed: boolean) => void
  reject: (error: unknown) => void
  timer?: number
  useReady?: boolean
}

function analysisComplete(progress: AssetAnalysisProgress) {
  return analysisPendingCount(progress) === 0 && progress.failed + progress.cancelled === 0
}

export function useAnalysisGateController(projectId: string | null, sessionId: string | null, importing: boolean) {
  const pending = useRef<PendingAnalysis | null>(null)
  const importingRef = useRef(importing)
  importingRef.current = importing
  const [progress, setProgress] = useState<AssetAnalysisProgress | null>(null)
  const [waiting, setWaiting] = useState(false)
  const [prompting, setPrompting] = useState(false)

  function finish(proceed: boolean) {
    const request = pending.current
    if (!request) return
    pending.current = null
    window.clearTimeout(request.timer)
    setProgress(null)
    setWaiting(false)
    setPrompting(false)
    request.resolve(proceed)
  }

  async function check(request: PendingAnalysis) {
    try {
      const next = await getAssetAnalysisProgress(request.projectId)
      if (pending.current !== request) return
      setProgress(next)
      if (!importingRef.current && analysisComplete(next)) {
        finish(true)
        return
      }
      if (!importingRef.current && request.useReady && next.readyVideo > 0) {
        finish(true)
        return
      }
      setPrompting(true)
      window.clearTimeout(request.timer)
      request.timer = window.setTimeout(() => void check(request), 1500)
    } catch (error) {
      if (pending.current !== request) return
      pending.current = null
      window.clearTimeout(request.timer)
      setProgress(null)
      setWaiting(false)
      setPrompting(false)
      request.reject(error)
    }
  }

  function waitForAnalysis() {
    if (!projectId) return Promise.resolve(true)
    return new Promise<boolean>((resolve, reject) => {
      const request = { projectId, resolve, reject }
      pending.current = request
      setWaiting(true)
      setPrompting(false)
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
    setPrompting(false)
  }, [projectId, sessionId])

  return {
    progress,
    waiting,
    prompting,
    waitForAnalysis,
    cancel: () => finish(false),
    useReady: () => {
      const request = pending.current
      if (!request) return
      request.useReady = true
      void check(request)
    },
  }
}
