// 当前项目设置：候选召回比例与预览缓存维护。
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { clearPreviewCache, getCandidateScoreFirstSlots, getPreviewCacheStatus, setCandidateScoreFirstSlots } from '../lib/local-store'
import type { PreviewCacheStatus } from '../lib/local-store'

type ProjectSettingsModalProps = {
  open: boolean
  projectId: string | null
  projectName: string | null
  onClose: () => void
}

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`
}

export function ProjectSettingsModal({ open, projectId, projectName, onClose }: ProjectSettingsModalProps) {
  const dialog = useRef<HTMLDialogElement>(null)
  useLayoutEffect(() => {
    const element = dialog.current
    if (open) element?.showModal()
    return () => element?.close()
  }, [open])
  const [status, setStatus] = useState<PreviewCacheStatus | null>(null)
  const [scoreFirstSlots, setScoreFirstSlots] = useState<number | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open || !projectId) {
      setStatus(null)
      setScoreFirstSlots(null)
      setError(null)
      return
    }
    let active = true
    setBusy(true)
    setError(null)
    void Promise.all([getPreviewCacheStatus(projectId), getCandidateScoreFirstSlots(projectId)])
      .then(([nextStatus, nextSlots]) => {
        if (active) {
          setStatus(nextStatus)
          setScoreFirstSlots(nextSlots)
        }
      })
      .catch(() => {
        if (active) setError('无法读取项目设置。')
      })
      .finally(() => {
        if (active) setBusy(false)
      })
    return () => {
      active = false
    }
  }, [open, projectId])

  if (!open) return null

  async function handleClear() {
    if (!projectId || busy) return
    const confirmed = window.confirm(
      '清理当前项目的预览缓存？\n\n已生成的预览仍可重新渲染，不会删除素材或剪辑结果。',
    )
    if (!confirmed) return
    setBusy(true)
    setError(null)
    try {
      const next = await clearPreviewCache(projectId, true)
      setStatus(next)
    } catch {
      setError('清理失败，请稍后重试。')
    } finally {
      setBusy(false)
    }
  }

  async function handleScoreFirstSlots(next: number) {
    if (!projectId) return
    setBusy(true)
    setError(null)
    try {
      setScoreFirstSlots(await setCandidateScoreFirstSlots(projectId, next))
    } catch {
      setError('保存候选比例失败。')
    } finally {
      setBusy(false)
    }
  }

  return (
    <dialog ref={dialog} className="settings-dialog" aria-label="项目设置" onCancel={(event) => { event.preventDefault(); dialog.current?.close(); onClose() }}>
      <section className="provider-modal">
        <button className="close-button" onClick={() => { dialog.current?.close(); onClose() }} aria-label="关闭">×</button>
        <span className="eyebrow">PROJECT</span>
        <h2>项目设置</h2>
        <p>{projectName ? `项目：${projectName}` : '请先选择一个项目。'}</p>

        <label className="provider-option chosen">
          <span>
            <strong>候选镜头：综合分优先数量</strong>
            <small>每拍最多 9 条。其余名额从画面、语义和关键词高分候选中补入；下次生成生效。</small>
          </span>
          <select
            aria-label="综合分优先数量"
            value={scoreFirstSlots ?? 5}
            onChange={(event) => void handleScoreFirstSlots(Number(event.target.value))}
            disabled={!projectId || busy || scoreFirstSlots === null}
          >
            {[3, 4, 5, 6, 7, 8, 9].map((count) => <option key={count} value={count}>{count} 条</option>)}
          </select>
        </label>

        <div className="provider-option chosen">
          <span>
            <strong>预览缓存</strong>
            <small>
              单个项目最多约 {status ? formatBytes(status.limitBytes) : '2 GB'}。超出后自动淘汰最旧中间文件。
            </small>
          </span>
          <b>{status ? formatBytes(status.bytesUsed) : busy ? '读取中' : '—'}</b>
        </div>
        {status && (
          <p className="oauth-status">
            {status.fileCount} 个缓存文件 · 上限 {formatBytes(status.limitBytes)}
          </p>
        )}
        {error && <p className="oauth-status">{error}</p>}
        <button
          className="outline-button modal-button"
          onClick={() => void handleClear()}
          disabled={!projectId || busy}
        >
          {busy ? '处理中…' : '清理预览缓存'}
        </button>
      </section>
    </dialog>
  )
}
