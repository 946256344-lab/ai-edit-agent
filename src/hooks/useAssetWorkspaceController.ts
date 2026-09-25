// 素材工作区 controller：拥有目录选择、分页轮询、证据查看、健康检查和显式重链路状态。
import { useAssetAnalysisController } from './useAssetAnalysisController'
import { useAssetLibraryEditController } from './useAssetLibraryEditController'
import { useEffect, useState } from 'react'
import type { RefObject } from 'react'
import { convertFileSrc } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'
import { listen } from '@tauri-apps/api/event'
import type { AssetView } from '../components/asset-workspace/AssetBrowser'
import { analysisPendingCount, assetAnalysisState, EMPTY_ANALYSIS_PROGRESS } from '../lib/asset-analysis'
import { messages } from '../lib/i18n'
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
  retryAssetAnalysisBatch,
  startAssetHealthScan,
} from '../lib/local-store'
import type {
  AgentEditEvent,
  AssetEvidence,
  AssetAnalysisState,
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

const EMPTY_PAGE: Pick<AssetPage, 'total' | 'directories' | 'unfiledCount' | 'counts' | 'progress'> = {
  total: 0,
  directories: [],
  unfiledCount: 0,
  counts: { total: 0, ready: 0, analyzing: 0, queued: 0, failed: 0, visualPending: 0, segmentPending: 0 },
  progress: EMPTY_ANALYSIS_PROGRESS,
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
    analysisState: assetAnalysisState(asset),
    analysisCancelled: asset.analysisCancelled,
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
  const [analysisFilter, setAnalysisFilter] = useState<AssetAnalysisState | null>(null)
  const [importingProjectId, setImportingProjectId] = useState<string | null>(null)
  const [retryingProjectId, setRetryingProjectId] = useState<string | null>(null)
  const importing = importingProjectId !== null && importingProjectId === options.projectId
  const retrying = retryingProjectId !== null && retryingProjectId === options.projectId
  const [analysisNotice, setAnalysisNotice] = useState<string | null>(null)

  const analysis = useAssetAnalysisController(options.projectId, () => setPageRevision(value => value + 1))
  const editing = useAssetLibraryEditController(options.projectId, assets, () => { setEvidence(null); setPageRevision(value => value + 1) })

  useEffect(() => {
    if (!options.desktopRuntime || !options.projectId) return
    const projectId = options.projectId
    let cancelled = false
    let timer: number | undefined
    const refreshAssets = () => {
      void listAssetPage(projectId, {
        directoryKey: directoryKey === 'all' ? undefined : directoryKey,
        analysisState: analysisFilter ?? undefined,
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
            progress: nextPage.progress,
          }
          setPage((current) => JSON.stringify(current) === JSON.stringify(nextState) ? current : nextState)
          if (analysisPendingCount(nextPage.progress) > 0) {
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
  }, [analysisFilter, directoryKey, options.activeProjectRef, options.desktopRuntime, options.projectId, pageRevision])

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
    editing.actions.clear()
    setAssets([])
    setPage(EMPTY_PAGE)
    setDirectoryKey('all')
    setHealth(null)
    setRelinkPreview(null)
    setRelinkSourceDirectory(null)
    setEvidence(null)
    setAnalysisFilter(null)
    setAnalysisNotice(null)
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
    setImportingProjectId(context.projectId)
    setAnalysisNotice(null)
    try {
      const imported = await importAssets(context.projectId, sources)
      if (options.activeProjectRef.current === context.projectId) {
        analysis.showImported(context.projectId, imported.map(asset => asset.id))
        setPageRevision((value) => value + 1)
        setHealthRevision((value) => value + 1)
      }
      await options.appendAgentMessage(
        context.conversationId,
        context.sessionId,
        messages().assets.imported(imported.length),
      )
      await options.refreshEditingSessions(context.projectId)
    } catch {
      if (options.activeProjectRef.current === context.projectId) setAnalysisNotice(messages().assets.importFilesFailed)
    } finally {
      setImportingProjectId(current => current === context.projectId ? null : current)
    }
  }

  async function importSelectedFolder() {
    if (!options.desktopRuntime) return
    const context = await currentOrNewEditingContext()
    const selected = await open({ directory: true, multiple: false })
    if (!selected || Array.isArray(selected)) return
    setImportingProjectId(context.projectId)
    setAnalysisNotice(null)
    try {
      const imported = await importAssetFolder(context.projectId, selected)
      if (options.activeProjectRef.current === context.projectId) {
        analysis.showImported(context.projectId, imported.map(asset => asset.id))
        setPageRevision((value) => value + 1)
        setHealthRevision((value) => value + 1)
      }
      await options.appendAgentMessage(
        context.conversationId,
        context.sessionId,
        messages().assets.importedFolder(imported.length),
      )
      await options.refreshEditingSessions(context.projectId)
    } catch {
      if (options.activeProjectRef.current === context.projectId) setAnalysisNotice(messages().assets.importFolderFailed)
    } finally {
      setImportingProjectId(current => current === context.projectId ? null : current)
    }
  }

  async function retryFailed() {
    const projectId = options.projectId
    if (!projectId || retrying) return
    setRetryingProjectId(projectId)
    setAnalysisNotice(null)
    try {
      // 先收集完整失败集合，再分批重试；避免状态变化使分页漏项。
      const ids: string[] = []
      let offset = 0
      let total: number
      do {
        const failed = await listAssetPage(projectId, { analysisState: 'failed', offset, limit: 200 })
        ids.push(...failed.items.map(asset => asset.id))
        total = failed.total
        offset += 200
      } while (offset < total)
      let updated = 0
      for (let index = 0; index < ids.length; index += 200) {
        const result = await retryAssetAnalysisBatch(projectId, ids.slice(index, index + 200))
        updated += result.updatedCount
      }
      if (options.activeProjectRef.current === projectId) {
        setAnalysisNotice(updated ? messages().assets.requeued(updated) : messages().assets.nothingToRetry)
      }
    } catch {
      if (options.activeProjectRef.current === projectId) setAnalysisNotice(messages().assets.retryFailed)
    } finally {
      setRetryingProjectId(current => current === projectId ? null : current)
      if (options.activeProjectRef.current === projectId) setPageRevision(value => value + 1)
    }
  }

  async function openRelink() {
    if (!options.desktopRuntime || !options.projectId) return
    const selected = await open({ directory: true, multiple: false, title: messages().assets.pickRelinkRoot })
    if (!selected || Array.isArray(selected)) return
    const nextPreview = await previewAssetRelink(options.projectId, selected)
    setRelinkPreview(nextPreview)
    setRelinkSourceDirectory(selected)
    if (!nextPreview.matches.length) {
      window.alert(messages().assets.relinkNone)
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
    window.alert(messages().assets.relinked(result.relinkedCount))
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
    editing.actions.clear()
    setEvidence(null)
    if (path === directoryKey) return
    setAssets([])
    setDirectoryKey(path)
  }

  return {
    assets,
    page,
    reset,
    analysis,
    model: {
      projectId: options.projectId,
      storeReady: options.storeReady,
      page: { total: page.total, counts: page.counts, progress: page.progress },
      analysisFilter,
      importing,
      retrying,
      analysisNotice: analysis.model.notice ?? analysisNotice,
      analysisBusy: analysis.model.busy,
      editing: editing.model,
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
      editing: editing.actions,
      cancelAnalysis: analysis.actions.cancel,
      resumeAnalysis: analysis.actions.resume,
      selectAnalysisFilter: (state: AssetAnalysisState | null) => { editing.actions.clear(); setAssets([]); setAnalysisFilter(state) },
      retryFailed: () => void retryFailed(),
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
