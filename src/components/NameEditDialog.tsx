// 名称编辑弹窗：项目与剪辑会话共用，只负责输入和保存反馈。
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { useI18n } from '../lib/i18n'

type NameEditDialogProps = {
  open: boolean
  label: string
  initialValue: string
  onClose: () => void
  onSave: (value: string) => Promise<void>
}

export function NameEditDialog({ open, label, initialValue, onClose, onSave }: NameEditDialogProps) {
  const dialog = useRef<HTMLDialogElement>(null)
  const { t } = useI18n()
  const input = useRef<HTMLInputElement>(null)
  const [value, setValue] = useState(initialValue)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    setValue(initialValue)
    setError(null)
  }, [initialValue, open])

  useLayoutEffect(() => {
    const element = dialog.current
    if (open) {
      element?.showModal()
      requestAnimationFrame(() => input.current?.select())
    }
    return () => element?.close()
  }, [open])

  if (!open) return null

  async function handleSave() {
    const next = value.trim()
    if (!next || saving) return
    setSaving(true)
    setError(null)
    try {
      await onSave(next)
      dialog.current?.close()
      onClose()
    } catch {
      setError(t.nameEdit.saveFailed)
    } finally {
      setSaving(false)
    }
  }

  return (
    <dialog ref={dialog} className="settings-dialog name-edit-dialog" aria-label={t.nameEdit.title(label)} onCancel={(event) => { event.preventDefault(); onClose() }}>
      <form className="provider-modal" onSubmit={(event) => { event.preventDefault(); void handleSave() }}>
        <button type="button" className="close-button" onClick={onClose} aria-label={t.common.close}>×</button>
        <span className="eyebrow">EDIT NAME</span>
        <h2>{t.nameEdit.title(label)}</h2>
        <label className="name-edit-field">
          <span>{t.nameEdit.name}</span>
          <input ref={input} value={value} onChange={(event) => setValue(event.target.value)} />
        </label>
        {error && <p className="oauth-status">{error}</p>}
        <div className="name-edit-actions">
          <button type="button" className="outline-button" onClick={onClose}>{t.common.cancel}</button>
          <button type="submit" className="primary-button" disabled={!value.trim() || saving}>{saving ? t.common.savingEllipsis : t.common.save}</button>
        </div>
      </form>
    </dialog>
  )
}
