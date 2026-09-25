// 素材库编辑状态：批量选择、显示名修改和移除确认；不修改源文件。
import { useState } from 'react'
import { removeLibraryAssets, renameLibraryAsset } from '../lib/local-store'
import { messages } from '../lib/i18n'
import type { AssetView } from '../components/asset-workspace/AssetBrowser'

export function useAssetLibraryEditController(projectId: string | null, assets: AssetView[], onChanged: () => void) {
  const [selected, setSelected] = useState<string[]>([])
  const [rename, setRename] = useState<{ id: string; name: string } | null>(null)
  const [removing, setRemoving] = useState(false)
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState<string | null>(null)
  const selectedIds = selected.filter(id => assets.some(asset => asset.id === id))
  function clear() { setSelected([]); setRename(null); setRemoving(false); setNotice(null) }
  async function save() {
    if (!projectId) return
    setBusy(true)
    setNotice(null)
    try {
      if (rename) await renameLibraryAsset(projectId, rename.id, rename.name)
      else await removeLibraryAssets(projectId, selectedIds)
      clear()
      onChanged()
    } catch { setNotice(rename ? messages().assets.renameFailed : messages().assets.removeFailed) }
    finally { setBusy(false) }
  }
  return {
    model: { selectedIds, rename, removing, busy, notice },
    actions: {
      clear,
      toggle: (id: string) => setSelected(current => current.includes(id) ? current.filter(value => value !== id) : [...current, id]),
      selectAll: () => setSelected(selectedIds.length === assets.length ? [] : assets.map(asset => asset.id)),
      rename: (id: string) => { const asset = assets.find(item => item.id === id); if (asset) { setNotice(null); setRename({ id, name: asset.name }) } },
      setName: (name: string) => setRename(current => current ? { ...current, name } : null),
      remove: () => { setNotice(null); setRemoving(true) },
      close: () => { setRename(null); setRemoving(false); setNotice(null) },
      save: () => void save(),
    },
  }
}

export type AssetLibraryEditController = ReturnType<typeof useAssetLibraryEditController>
