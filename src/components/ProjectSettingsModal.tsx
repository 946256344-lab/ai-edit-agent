// 当前项目维护弹窗：预览缓存占用与显式清理，不改 Provider 或素材路径。
import { useEffect, useState } from 'react'
import { clearPreviewCache, getPreviewCacheStatus } from '../lib/local-store'
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
  const [status, setStatus] = useState<PreviewCacheStatus | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open || !projectId) {
      setStatus(null)
      setError(null)
      return
    }
    let active = true
    setBusy(true)
    setError(null)
    void getPreviewCacheStatus(projectId)
      .then((next) => {
        if (active) setStatus(next)
      })
      .catch(() => {
        if (active) setError('无法读取预览缓存占用。')
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

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-label="项目设置">
      <section className="provider-modal">
        <button className="close-button" onClick={onClose} aria-label="关闭">x</button>
        <span className="eyebrow">PROJECT</span>
        <h2>项目设置</h2>
        <p>{projectName ? `当前项目：${projectName}` : '请先选择一个项目。'}</p>

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
    </div>
  )
}
