// 素材工作区 controller：拥有目录选择、分页轮询、证据查看、健康检查和显式重链路状态。
import { useEffect, useState } from 'react'
import type { RefObject } from 'react'
import { convertFileSrc } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { listen } from '@tauri-apps/api/event'
import type { AssetView } from '../components/asset-workspace/AssetBrowser'
import type { EditingSessionView } from '../components/workspace-types'
import {
  cancelAssetHealthScan,
  confirmAssetRelink,
  getAssetEvidence,
  getAssetHealthScanSummary,
  importAssetFolder,
  importAssets,
  isDesktopRuntime,
  listAssetPage,
  previewAssetRelink,
  startAssetHealthScan,
} from '../lib/local-store'
import type {
  AgentEditEvent,
  AssetEvidence,
  AssetHealthScanSummary,
  AssetPage,
  AssetRelinkPreview,
  StoredAsset,
} from '../lib/local-store'

type EditingContext = {
  projectId: string
  sessionId: string
  conversationId: string
}

type AssetWorkspaceControllerOptions = {
  desktopRuntime: boolean
  storeReady: boolean
  projectId: string | null
  session: EditingSessionView | undefined
  activeProjectRef: RefObject<string | null>
  ensureEditingSession: () => Promise<EditingContext>
  appendAgentMessage: (conversationId: string, sessionId: string, content: string) => Promise<void>
  refreshEditingSessions: (projectId: string) => Promise<unknown>
}

const EMPTY_PAGE: Pick<AssetPage, 'total' | 'directories' | 'unfiledCount' | 'counts'> = {
  total: 0,
  directories: [],
  unfiledCount: 0,
  counts: { total: 0, ready: 0, analyzing: 0, queued: 0, failed: 0, visualPending: 0, segmentPending: 0 },
}

function formatDuration(durationMs: number | null) {
  if (durationMs === null) return ''
  const seconds = Math.floor(durationMs / 1000)
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
}

function toAsset(asset: StoredAsset): AssetView {
  const kind = asset.kind === 'video' || asset.kind === 'image' || asset.kind === 'audio' ? asset.kind : 'other'
  return {
    id: asset.id,
    name: asset.displayName,
    folderName: asset.folderName,
    relativePath: asset.relativePath,
    kind,
    duration: formatDuration(asset.durationMs),
    status: asset.analysisStatus === 'ready'
      ? 'ready'
      : asset.analysisStatus === 'failed'
        ? 'failed'
        : asset.analysisStatus === 'queued'
          ? 'queued'
          : 'analyzing',
    visualStatus: asset.visualAnalysisStatus,
    sourceHealthStatus: asset.sourceHealthStatus,
    thumbnailUrl: asset.thumbnailPath && isDesktopRuntime() ? convertFileSrc(asset.thumbnailPath) : null,
  }
}

/**
 * Owns the project-scoped asset projection and explicit import/recovery
 * actions. It reads safe Tauri projections rather than walking source paths;
 * folder expansion remains local to AssetDirectoryTree.
 */
export function useAssetWorkspaceController(options: AssetWorkspaceControllerOptions) {
  const [assets, setAssets] = useState<AssetView[]>([])
  const [page, setPage] = useState(EMPTY_PAGE)
  const [pageRevision, setPageRevision] = useState(0)
  const [healthRevision, setHealthRevision] = useState(0)
  const [health, setHealth] = useState<AssetHealthScanSummary | null>(null)
  const [relinkPreview, setRelinkPreview] = useState<AssetRelinkPreview | null>(null)
  const [relinkSourceDirectory, setRelinkSourceDirectory] = useState<string | null>(null)
  const [directoryKey, setDirectoryKey] = useState('all')
  const [evidence, setEvidence] = useState<AssetEvidence | null>(null)

  useEffect(() => {
    if (!options.desktopRuntime || !options.projectId) return
    const projectId = options.projectId
    let cancelled = false
    let timer: number | undefined
    const refreshAssets = () => {
      void listAssetPage(projectId, {
        directoryKey: directoryKey === 'all' ? undefined : directoryKey,
        offset: 0,
        limit: 100,
      }).then((nextPage) => {
        if (!cancelled && options.activeProjectRef.current === projectId) {
          const nextAssets = nextPage.items.map(toAsset)
          setAssets((current) => JSON.stringify(current) === JSON.stringify(nextAssets) ? current : nextAssets)
          const nextState = {
            total: nextPage.total,
            directories: nextPage.directories,
            unfiledCount: nextPage.unfiledCount,
            counts: nextPage.counts,
          }
          setPage((current) => JSON.stringify(current) === JSON.stringify(nextState) ? current : nextState)
          if (nextPage.counts.queued + nextPage.counts.analyzing + nextPage.counts.visualPending > 0) {
            timer = window.setTimeout(refreshAssets, 1500)
          }
        }
      }).catch(() => undefined)
    }
    refreshAssets()
    return () => {
      cancelled = true
      window.clearTimeout(timer)
    }
  }, [directoryKey, options.activeProjectRef, options.desktopRuntime, options.projectId, pageRevision])

  useEffect(() => {
    if (!options.desktopRuntime || !options.projectId) return
    const projectId = options.projectId
    let cancelled = false
    let timer: number | undefined
    // 只在健康计数或任务状态变化时刷新素材页；首次观察与空闲重复摘要不 bump。
    let previousKey: string | null = null
    const refresh = () => void getAssetHealthScanSummary(projectId)
      .then((summary) => {
        if (cancelled || options.activeProjectRef.current !== projectId) return
        setHealth((current) =>
          JSON.stringify(current) === JSON.stringify(summary) ? current : summary,
        )
        const key = JSON.stringify({
          unchecked: summary.unchecked,
          online: summary.online,
          missing: summary.missing,
          changed: summary.changed,
          unreadable: summary.unreadable,
          checked: summary.checked,
          activeTaskId: summary.activeTaskId,
          activeTaskStatus: summary.activeTaskStatus,
        })
        if (previousKey !== null && previousKey !== key) {
          setPageRevision((value) => value + 1)
        }
        previousKey = key
        if (summary.activeTaskStatus === 'queued' || summary.activeTaskStatus === 'running') {
          timer = window.setTimeout(refresh, 2000)
        }
      })
      .catch(() => undefined)
    refresh()
    return () => {
      cancelled = true
      window.clearTimeout(timer)
    }
  }, [options.activeProjectRef, options.desktopRuntime, options.projectId, healthRevision])

  useEffect(() => {
    if (!options.desktopRuntime || !options.projectId) return
    const listener = listen<AgentEditEvent>('agent-edit-completed', () => {
      setPageRevision((value) => value + 1)
      setHealthRevision((value) => value + 1)
    })
    const assetsListener = listen<string>('assets-changed', ({ payload }) => {
      if (payload === options.projectId) setPageRevision((value) => value + 1)
    })
    return () => {
      void listener.then((unlisten) => unlisten())
      void assetsListener.then((unlisten) => unlisten())
    }
  }, [options.desktopRuntime, options.projectId])

  function reset() {
    setAssets([])
    setPage(EMPTY_PAGE)
    setDirectoryKey('all')
    setHealth(null)
    setRelinkPreview(null)
    setRelinkSourceDirectory(null)
    setEvidence(null)
  }

  function currentOrNewEditingContext() {
    return options.projectId && options.session?.conversationId
      ? Promise.resolve({
          projectId: options.projectId,
          conversationId: options.session.conversationId,
          sessionId: options.session.id,
        })
      : options.ensureEditingSession()
  }

  async function importSelectedAssets() {
    if (!options.desktopRuntime) return
    const context = await currentOrNewEditingContext()
    const selected = await open({
      multiple: true,
      filters: [{
        name: 'Media',
        extensions: ['mp4', 'mov', 'mkv', 'avi', 'webm', 'm4v', 'jpg', 'jpeg', 'png', 'webp', 'bmp', 'gif', 'mp3', 'wav', 'aac', 'm4a', 'flac', 'ogg'],
      }],
    })
    if (!selected) return
    const sources = Array.isArray(selected) ? selected : [selected]
    const imported = await importAssets(context.projectId, sources)
    if (options.activeProjectRef.current === context.projectId) {
      setPageRevision((value) => value + 1)
      setHealthRevision((value) => value + 1)
    }
    await options.appendAgentMessage(
      context.conversationId,
      context.sessionId,
      `已将 ${imported.length} 个素材加入本地分析队列。分析完成前不会影响当前故事板。`,
    )
    await options.refreshEditingSessions(context.projectId)
  }

  async function importSelectedFolder() {
    if (!options.desktopRuntime) return
    const context = await currentOrNewEditingContext()
    const selected = await open({ directory: true, multiple: false })
    if (!selected || Array.isArray(selected)) return
    const imported = await importAssetFolder(context.projectId, selected)
    if (options.activeProjectRef.current === context.projectId) {
      setPageRevision((value) => value + 1)
      setHealthRevision((value) => value + 1)
    }
    await options.appendAgentMessage(
      context.conversationId,
      context.sessionId,
      `已从文件夹导入 ${imported.length} 个素材。仅支持的媒体文件会加入本地分析队列。`,
    )
    await options.refreshEditingSessions(context.projectId)
  }

  async function openRelink() {
    if (!options.desktopRuntime || !options.projectId) return
    const selected = await open({ directory: true, multiple: false, title: '选择新的素材根目录' })
    if (!selected || Array.isArray(selected)) return
    const nextPreview = await previewAssetRelink(options.projectId, selected)
    setRelinkPreview(nextPreview)
    setRelinkSourceDirectory(selected)
    if (!nextPreview.matches.length) {
      window.alert('没有找到可安全重链路的素材。请选择保留原有文件夹结构的素材根目录。')
    }
  }

  async function confirmRelink() {
    if (!options.desktopRuntime || !options.projectId || !relinkSourceDirectory || !relinkPreview) return
    const result = await confirmAssetRelink(
      options.projectId,
      relinkSourceDirectory,
      relinkPreview.matches.map((match) => match.assetId),
      true,
    )
    if (options.activeProjectRef.current === options.projectId) {
      setPageRevision((value) => value + 1)
      setHealthRevision((value) => value + 1)
    }
    window.alert(`已重新链路 ${result.relinkedCount} 个素材并保留分析信息。`)
    cancelRelink()
  }

  function cancelRelink() {
    setRelinkPreview(null)
    setRelinkSourceDirectory(null)
  }

  async function inspectAsset(assetId: string) {
    if (!options.desktopRuntime) return
    const projectId = options.activeProjectRef.current
    try {
      const nextEvidence = await getAssetEvidence(assetId)
      if (options.activeProjectRef.current === projectId) setEvidence(nextEvidence)
    } catch {
      if (options.activeProjectRef.current === projectId) setEvidence(null)
    }
  }

  function selectDirectory(path: string) {
    setEvidence(null)
    if (path === directoryKey) return
    setAssets([])
    setDirectoryKey(path)
  }

  return {
    assets,
    page,
    reset,
    model: {
      projectId: options.projectId,
      storeReady: options.storeReady,
      page: { total: page.total, counts: page.counts },
      directories: page.directories,
      unfiledAssetCount: page.unfiledCount,
      assets,
      selectedDirectoryKey: directoryKey,
      health,
      relinkPreview,
      hasRelinkSource: Boolean(relinkSourceDirectory),
      evidence,
    },
    actions: {
      selectDirectory,
      inspectAsset: (assetId: string) => void inspectAsset(assetId),
      closeEvidence: () => setEvidence(null),
      importFiles: () => void importSelectedAssets(),
      importFolder: () => void importSelectedFolder(),
      startHealthScan: () => { if (options.projectId) void startAssetHealthScan(options.projectId).then(() => setHealthRevision((value) => value + 1)) },
      cancelHealthScan: (taskId: string) => { if (options.projectId) void cancelAssetHealthScan(options.projectId, taskId).then(() => setHealthRevision((value) => value + 1)) },
      openRelink: () => void openRelink(),
      confirmRelink: () => void confirmRelink(),
      cancelRelink,
    },
  }
}

export type AssetWorkspaceController = ReturnType<typeof useAssetWorkspaceController>
