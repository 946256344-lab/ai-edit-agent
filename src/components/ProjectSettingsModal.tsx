// 当前项目设置：候选召回比例与预览缓存维护。
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { clearPreviewCache, getCandidateScoreFirstSlots, getPreviewCacheStatus, setCandidateScoreFirstSlots } from '../lib/local-store'
import type { PreviewCacheStatus } from '../lib/local-store'
import { messages, useI18n } from '../lib/i18n'

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
  const { t } = useI18n()
  const copy = t.projectSettings
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
        if (active) setError(messages().projectSettings.loadFailed)
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
    const confirmed = window.confirm(copy.clearConfirm)
    if (!confirmed) return
    setBusy(true)
    setError(null)
    try {
      const next = await clearPreviewCache(projectId, true)
      setStatus(next)
    } catch {
      setError(copy.clearFailed)
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
      setError(copy.slotsSaveFailed)
    } finally {
      setBusy(false)
    }
  }

  return (
    <dialog ref={dialog} className="settings-dialog" aria-label={copy.title} onCancel={(event) => { event.preventDefault(); dialog.current?.close(); onClose() }}>
      <section className="provider-modal">
        <button className="close-button" onClick={() => { dialog.current?.close(); onClose() }} aria-label={t.common.close}>×</button>
        <span className="eyebrow">PROJECT</span>
        <h2>{copy.title}</h2>
        <p>{projectName ? copy.projectLine(projectName) : copy.selectProjectFirst}</p>

        <label className="provider-option chosen">
          <span>
            <strong>{copy.slotsTitle}</strong>
            <small>{copy.slotsHint}</small>
          </span>
          <select
            aria-label={copy.slotsAria}
            value={scoreFirstSlots ?? 5}
            onChange={(event) => void handleScoreFirstSlots(Number(event.target.value))}
            disabled={!projectId || busy || scoreFirstSlots === null}
          >
            {[3, 4, 5, 6, 7, 8, 9].map((count) => <option key={count} value={count}>{copy.slotsOption(count)}</option>)}
          </select>
        </label>

        <div className="provider-option chosen">
          <span>
            <strong>{copy.cacheTitle}</strong>
            <small>
              {copy.cacheHint(status ? formatBytes(status.limitBytes) : '2 GB')}
            </small>
          </span>
          <b>{status ? formatBytes(status.bytesUsed) : busy ? copy.reading : '—'}</b>
        </div>
        {status && (
          <p className="oauth-status">
            {copy.cacheFiles(status.fileCount, formatBytes(status.limitBytes))}
          </p>
        )}
        {error && <p className="oauth-status">{error}</p>}
        <button
          className="outline-button modal-button"
          onClick={() => void handleClear()}
          disabled={!projectId || busy}
        >
          {busy ? t.common.processing : copy.clearCache}
        </button>
      </section>
    </dialog>
  )
}
