// 素材管理组合组件：连接目录树、直属素材列表、证据 Inspector 和来源恢复面板。
import { useMemo } from 'react'
import type { AssetDirectory, AssetEvidence, AssetHealthScanSummary, AssetPage, AssetRelinkPreview } from '../lib/local-store'
import { AssetBrowser } from './asset-workspace/AssetBrowser'
import type { AssetView } from './asset-workspace/AssetBrowser'
import { AssetDirectoryTree } from './asset-workspace/AssetDirectoryTree'
import { AssetEvidenceInspector } from './asset-workspace/AssetEvidenceInspector'
import { AssetSourceRecovery } from './asset-workspace/AssetSourceRecovery'
import { buildAssetDirectoryTree } from './asset-workspace/asset-directory-model'

export type AssetWorkspaceModel = {
  projectId: string | null
  storeReady: boolean
  page: Pick<AssetPage, 'total' | 'counts'>
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

function directoryBreadcrumb(directoryKey: string) {
  if (directoryKey === 'all') return '项目中的全部素材'
  if (directoryKey === '__unfiled__') return '未归类素材'
  return directoryKey.split('\\').join(' / ')
}

export function AssetManagementPanel({ model, actions }: { model: AssetWorkspaceModel; actions: AssetWorkspaceActions }) {
  const tree = useMemo(() => buildAssetDirectoryTree(model.directories), [model.directories])
  const selectedNode = tree.nodes.get(model.selectedDirectoryKey)
  const currentTitle = model.selectedDirectoryKey === 'all'
    ? '全部素材'
    : model.selectedDirectoryKey === '__unfiled__'
      ? '未归类素材'
      : selectedNode?.name ?? '素材目录'
  const projectReady = Boolean(model.projectId && model.storeReady)

  return (
    <section className="asset-workbench">
      <header className="asset-workbench__header">
        <div>
          <span className="panel-kicker">素材</span>
          <strong>{model.page.counts.total} 个本地素材</strong>
          <p>{model.projectId ? '按导入时的本地目录浏览。这里不承担手工素材编目。' : '请选择项目后导入素材。'}</p>
        </div>
        <div className="asset-workbench__actions">
          <button className="import-button" onClick={actions.importFiles} disabled={!projectReady}>导入文件</button>
          <button className="import-button" onClick={actions.importFolder} disabled={!projectReady}>导入文件夹</button>
        </div>
      </header>

      <div className="asset-workbench__grid">
        <aside className="asset-workbench__left">
          <section className="asset-metrics" aria-label="素材分析摘要">
            <article><b>{model.page.counts.ready}</b><span>已分析</span></article>
            <article><b>{model.page.counts.analyzing}</b><span>分析中</span></article>
            <article><b>{model.page.counts.queued}</b><span>排队中</span></article>
            <article><b>{model.page.counts.failed}</b><span>失败</span></article>
          </section>
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
            breadcrumb={directoryBreadcrumb(model.selectedDirectoryKey)}
            matchingAssetCount={model.page.total}
            assets={model.assets}
            onInspect={actions.inspectAsset}
          />
        </main>

        <aside className="asset-workbench__right">
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
          <AssetEvidenceInspector evidence={model.evidence} onClose={actions.closeEvidence} />
          <footer className="asset-workbench__footer">
            <span className="state-dot working" />
            <p>源媒体保持在本机。浏览目录和证据不会改写原文件。</p>
          </footer>
        </aside>
      </div>
    </section>
  )
}
