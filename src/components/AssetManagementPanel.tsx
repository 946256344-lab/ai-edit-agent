// 素材管理组合组件：默认只展示导入、目录与素材状态；证据与恢复按需出现。
import type { AssetLibraryEditController } from '../hooks/useAssetLibraryEditController'
import { AssetEditDialog } from './asset-workspace/AssetEditDialog'
import { useMemo } from 'react'
import type { AssetAnalysisState, AssetDirectory, AssetEvidence, AssetHealthScanSummary, AssetPage, AssetRelinkPreview } from '../lib/local-store'
import { AssetAnalysisProgress } from './AssetAnalysisProgress'
import { AssetBrowser } from './asset-workspace/AssetBrowser'
import type { AssetView } from './asset-workspace/AssetBrowser'
import { AssetDirectoryTree } from './asset-workspace/AssetDirectoryTree'
import { AssetEvidenceInspector } from './asset-workspace/AssetEvidenceInspector'
import { AssetSourceRecovery } from './asset-workspace/AssetSourceRecovery'
import { buildAssetDirectoryTree } from './asset-workspace/asset-directory-model'
import { useI18n } from '../lib/i18n'
import type { Messages } from '../lib/i18n'

export type AssetWorkspaceModel = {
  projectId: string | null
  storeReady: boolean
  page: Pick<AssetPage, 'total' | 'counts' | 'progress'>
  analysisFilter: AssetAnalysisState | null
  importing: boolean
  retrying: boolean
  analysisNotice: string | null
  analysisBusy: boolean
  editing: AssetLibraryEditController['model']
  directories: AssetDirectory[]
  unfiledAssetCount: number
  assets: AssetView[]
  selectedDirectoryKey: string
  health: AssetHealthScanSummary | null
  relinkPreview: AssetRelinkPreview | null
  hasRelinkSource: boolean
  evidence: AssetEvidence | null
}

export type AssetWorkspaceActions = {
  selectAnalysisFilter: (state: AssetAnalysisState | null) => void
  retryFailed: () => void
  cancelAnalysis: () => void
  resumeAnalysis: () => void
  editing: AssetLibraryEditController['actions']
  selectDirectory: (directoryKey: string) => void
  inspectAsset: (assetId: string) => void
  closeEvidence: () => void
  importFiles: () => void
  importFolder: () => void
  startHealthScan: () => void
  cancelHealthScan: (taskId: string) => void
  openRelink: () => void
  confirmRelink: () => void
  cancelRelink: () => void
}

function directoryBreadcrumb(directoryKey: string, t: Messages) {
  if (directoryKey === 'all') return t.assets.allInProject
  if (directoryKey === '__unfiled__') return t.assets.unfiled
  return directoryKey.split('\\').join(' / ')
}

export function AssetManagementPanel({ model, actions }: { model: AssetWorkspaceModel; actions: AssetWorkspaceActions }) {
  const { t, locale } = useI18n()
  const copy = t.assets
  const tree = useMemo(() => buildAssetDirectoryTree(model.directories, locale), [model.directories, locale])
  const selectedNode = tree.nodes.get(model.selectedDirectoryKey)
  const currentTitle = model.selectedDirectoryKey === 'all'
    ? copy.all
    : model.selectedDirectoryKey === '__unfiled__'
      ? copy.unfiled
      : selectedNode?.name ?? copy.directoryFallback
  const projectReady = Boolean(model.projectId && model.storeReady && !model.importing)
  const healthIssues = model.health
    ? model.health.missing + model.health.changed + model.health.unreadable
    : 0
  const showRecovery = Boolean(
    model.relinkPreview
    || model.health?.activeTaskId
    || healthIssues > 0,
  )
  const showEvidence = Boolean(model.evidence)
  const showSidePanel = showRecovery || showEvidence

  return (
    <section className="asset-workbench">
      <header className="asset-workbench__header">
        <div>
          <span className="panel-kicker">{copy.kicker}</span>
          <strong>{copy.localCount(model.page.counts.total)}</strong>
          <p>{model.projectId ? copy.projectHint : copy.noProjectHint}</p>
        </div>
        <div className="asset-workbench__actions">
          <button className="import-button" onClick={actions.importFiles} disabled={!projectReady}>{copy.importFiles}</button>
          <button className="import-button" onClick={actions.importFolder} disabled={!projectReady}>{copy.importFolder}</button>
        </div>
      </header>

      <div className={`asset-workbench__grid ${showSidePanel ? '' : 'asset-workbench__grid--compact'} ${showEvidence ? 'asset-workbench__grid--inspecting' : ''}`}>
        <aside className="asset-workbench__left">
          <AssetDirectoryTree
            projectId={model.projectId}
            roots={tree.roots}
            selectedDirectoryKey={model.selectedDirectoryKey}
            totalAssetCount={model.page.counts.total}
            unfiledAssetCount={model.unfiledAssetCount}
            onSelectDirectory={actions.selectDirectory}
          />
        </aside>

        <main className="asset-workbench__center">
          <AssetBrowser
            title={currentTitle}
            breadcrumb={directoryBreadcrumb(model.selectedDirectoryKey, t)}
            isRoot={model.selectedDirectoryKey === 'all'}
            matchingAssetCount={model.page.total}
            assets={model.assets}
            filtered={model.analysisFilter !== null}
            onInspect={actions.inspectAsset}
            editing={{ model: model.editing, actions: actions.editing }}
            analysis={(model.page.progress.total > 0 || model.importing || model.analysisNotice) ? (
              <AssetAnalysisProgress
                placement="toolbar"
                progress={model.page.progress}
                selected={model.analysisFilter}
                onSelect={actions.selectAnalysisFilter}
                onRetry={actions.retryFailed}
                retrying={model.retrying}
                importing={model.importing}
                notice={model.analysisNotice}
                onCancel={actions.cancelAnalysis}
                onResume={actions.resumeAnalysis}
                busy={model.analysisBusy}
              />
            ) : undefined}
          />
        </main>

        {showSidePanel && (
          <aside className="asset-workbench__right">
            {showRecovery && (
              <AssetSourceRecovery
                projectReady={projectReady}
                health={model.health}
                relinkPreview={model.relinkPreview}
                hasRelinkSource={model.hasRelinkSource}
                onStartHealthScan={actions.startHealthScan}
                onCancelHealthScan={actions.cancelHealthScan}
                onOpenRelink={actions.openRelink}
                onConfirmRelink={actions.confirmRelink}
                onCancelRelink={actions.cancelRelink}
              />
            )}
            {model.evidence && (
              <AssetEvidenceInspector key={model.evidence.id} evidence={model.evidence} onClose={actions.closeEvidence} />
            )}
          </aside>
        )}
      </div>
      <AssetEditDialog model={model.editing} actions={actions.editing} />
    </section>
  )
}
