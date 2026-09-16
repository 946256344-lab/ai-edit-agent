// 导入分析弹窗与后台控制：跟踪本次导入，按实际完成速度估算剩余时间。
import { useEffect, useRef, useState } from 'react'
import { cancelAssetAnalysis, getAssetAnalysisProgress, resumeAssetAnalysis } from '../lib/local-store'
import { analysisPendingCount, EMPTY_ANALYSIS_PROGRESS } from '../lib/asset-analysis'

type Batch = { projectId: string; assetIds?: string[]; startedAt: number; initialCompleted: number }

export function useAssetAnalysisController(projectId: string | null, onChanged: () => void) {
  const [batch, setBatch] = useState<Batch | null>(null)
  const [open, setOpen] = useState(false)
  const [progress, setProgress] = useState(EMPTY_ANALYSIS_PROGRESS)
  const [remainingSeconds, setRemainingSeconds] = useState<number | null>(null)
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const [revision, setRevision] = useState(0)
  const currentProject = useRef(projectId)
  currentProject.current = projectId
  const changed = useRef(onChanged)
  changed.current = onChanged

  useEffect(() => {
    if (!batch || batch.projectId !== projectId) return
    let disposed = false
    let timer: number | undefined
    async function refresh() {
      try {
        const next = await getAssetAnalysisProgress(batch!.projectId, batch!.assetIds)
        if (disposed) return
        setProgress(next)
        const completed = next.ready + next.failed - batch!.initialCompleted
        const pending = analysisPendingCount(next)
        setRemainingSeconds(completed > 0 && pending > 0 ? Math.ceil((Date.now() - batch!.startedAt) / 1000 / completed * pending) : null)
        if (pending > 0) timer = window.setTimeout(() => void refresh(), 1500)
        else changed.current()
      } catch {
        if (!disposed) setNotice('暂时无法读取分析进度，请返回素材库查看。')
      }
    }
    void refresh()
    return () => { disposed = true; window.clearTimeout(timer) }
  }, [batch, projectId, revision])

  function showImported(projectId: string, assetIds: string[]) {
    setBatch({ projectId, assetIds, startedAt: Date.now(), initialCompleted: 0 })
    setProgress({ ...EMPTY_ANALYSIS_PROGRESS, total: assetIds.length, queued: assetIds.length })
    setRemainingSeconds(null)
    setNotice(null)
    setOpen(true)
  }

  async function control(action: 'cancel' | 'resume', inDialog = false) {
    if (!projectId || busy) return
    const targetProject = projectId
    const assetIds = inDialog && batch?.projectId === projectId ? batch.assetIds : undefined
    setBusy(true)
    setNotice(null)
    try {
      await (action === 'cancel' ? cancelAssetAnalysis : resumeAssetAnalysis)(projectId, assetIds)
      if (currentProject.current !== targetProject) return
      if (action === 'cancel') setNotice('已取消后续分析。当前调用结束后退出，已完成的结果保留。')
      else if (batch?.projectId === projectId) setBatch({ ...batch, startedAt: Date.now(), initialCompleted: progress.ready + progress.failed })
      setRevision(value => value + 1)
      changed.current()
    } catch {
      if (currentProject.current === targetProject) setNotice(action === 'cancel' ? '取消分析未完成，请重试。' : '继续分析未完成，请检查素材文件后重试。')
    } finally { setBusy(false) }
  }

  return {
    model: { open: open && batch?.projectId === projectId, progress, remainingSeconds, busy, notice },
    showImported,
    actions: {
      background: () => setOpen(false),
      cancel: () => void control('cancel'),
      resume: () => void control('resume'),
      cancelBatch: () => void control('cancel', true),
      resumeBatch: () => void control('resume', true),
    },
  }
}
