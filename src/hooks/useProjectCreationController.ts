// 新建项目草稿：命名、默认全选的共享子素材库，确认后才创建项目。
import { useState } from 'react'
import { listSharedLibraries } from '../lib/local-store'
import type { SharedLibrary } from '../lib/local-store'
import { messages } from '../lib/i18n'

export function useProjectCreationController(create: (name: string, libraryIds: string[]) => Promise<void>) {
  const [isOpen, setIsOpen] = useState(false)
  const [name, setName] = useState('')
  const [libraries, setLibraries] = useState<SharedLibrary[]>([])
  const [selectedIds, setSelectedIds] = useState<string[]>([])
  const [loading, setLoading] = useState(false)
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  async function open() {
    setName(''); setError(''); setLibraries([]); setSelectedIds([]); setLoading(true); setIsOpen(true)
    try {
      const items = await listSharedLibraries()
      setLibraries(items); setSelectedIds(items.map((item) => item.id))
    } catch { setError(messages().projectCreate.libraryLoadFailed) }
    finally { setLoading(false) }
  }
  async function submit() {
    setSaving(true); setError('')
    try { await create(name.trim(), selectedIds); setIsOpen(false) }
    catch { setError(messages().projectCreate.createFailed) }
    finally { setSaving(false) }
  }
  return {
    model: { isOpen, name, libraries, selectedIds, loading, saving, error },
    actions: {
      open, submit, setName,
      close: () => { if (!saving) setIsOpen(false) },
      toggle: (id: string) => setSelectedIds((ids) => ids.includes(id) ? ids.filter((value) => value !== id) : [...ids, id]),
      selectAll: () => setSelectedIds(libraries.map((item) => item.id)),
      clear: () => setSelectedIds([]),
    },
  }
}
