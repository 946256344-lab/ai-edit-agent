// 名称编辑弹窗：项目与剪辑会话共用，只负责输入和保存反馈。
import { useEffect, useLayoutEffect, useRef, useState } from 'react'

type NameEditDialogProps = {
  open: boolean
  label: string
  initialValue: string
  onClose: () => void
  onSave: (value: string) => Promise<void>
}

export function NameEditDialog({ open, label, initialValue, onClose, onSave }: NameEditDialogProps) {
  const dialog = useRef<HTMLDialogElement>(null)
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
      setError('保存失败，请稍后重试。')
    } finally {
      setSaving(false)
    }
  }

  return (
    <dialog ref={dialog} className="settings-dialog name-edit-dialog" aria-label={`重命名${label}`} onCancel={(event) => { event.preventDefault(); onClose() }}>
      <form className="provider-modal" onSubmit={(event) => { event.preventDefault(); void handleSave() }}>
        <button type="button" className="close-button" onClick={onClose} aria-label="关闭">×</button>
        <span className="eyebrow">EDIT NAME</span>
        <h2>重命名{label}</h2>
        <label className="name-edit-field">
          <span>名称</span>
          <input ref={input} value={value} onChange={(event) => setValue(event.target.value)} />
        </label>
        {error && <p className="oauth-status">{error}</p>}
        <div className="name-edit-actions">
          <button type="button" className="outline-button" onClick={onClose}>取消</button>
          <button type="submit" className="primary-button" disabled={!value.trim() || saving}>{saving ? '保存中…' : '保存'}</button>
        </div>
      </form>
    </dialog>
  )
}
