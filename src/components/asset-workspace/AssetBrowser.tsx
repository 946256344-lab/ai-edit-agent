// 显示当前目录的直属素材卡片；默认只展示用户可读状态，不承担目录过滤或后台轮询。
import type { AssetLibraryEditController } from '../../hooks/useAssetLibraryEditController'
import type { AssetAnalysisState, StoredAsset } from '../../lib/local-store'
import type { ReactNode } from 'react'
import { useI18n } from '../../lib/i18n'
import type { Messages } from '../../lib/i18n'

export type AssetView = {
  id: string
  name: string
  folderName: string | null
  relativePath: string | null
  kind: 'video' | 'image' | 'audio' | 'other'
  duration: string
  status: 'ready' | 'analyzing' | 'queued' | 'failed'
  visualStatus: 'queued' | 'running' | 'ready' | 'failed' | 'skipped'
  analysisState: AssetAnalysisState
  analysisCancelled: boolean
  sourceHealthStatus: StoredAsset['sourceHealthStatus']
  thumbnailUrl: string | null
}

function analysisStatusLabel(asset: AssetView, t: Messages) {
  if (asset.sourceHealthStatus === 'missing' || asset.sourceHealthStatus === 'unreadable') {
    return { tone: 'failed' as const, label: t.assets.unreadable }
  }
  if (asset.sourceHealthStatus === 'changed') {
    return { tone: 'failed' as const, label: t.assets.changed }
  }
  return { tone: asset.analysisState, label: asset.analysisCancelled ? t.assets.analysisCancelled : t.analysis.labels[asset.analysisState] }
}

function AssetCard({ asset, onInspect, editing }: { asset: AssetView; onInspect: (id: string) => void; editing: AssetLibraryEditController }) {
  const { t } = useI18n()
  const copy = t.assets
  const status = analysisStatusLabel(asset, t)

  return (
    <article className="asset-card">
      <label className="asset-select"><input type="checkbox" aria-label={copy.selectItem(asset.name)} checked={editing.model.selectedIds.includes(asset.id)} onChange={() => editing.actions.toggle(asset.id)} disabled={editing.model.busy} />{copy.select}</label>
      <div className={`asset-card-thumb asset-card-thumb-${asset.kind}`}>
        {asset.thumbnailUrl && <img src={asset.thumbnailUrl} alt="" loading="lazy" decoding="async" />}
        <span>{asset.kind === 'video' ? 'VIDEO' : asset.kind.toUpperCase()}</span>
        {asset.duration && <time>{asset.duration}</time>}
      </div>
      <div className="asset-card-body">
        <header>
          <strong title={asset.name}>{asset.name}</strong>
          <small>{asset.relativePath ?? asset.folderName ?? copy.unfiled}</small>
        </header>
        <div className="asset-chip-row">
          <span className={`asset-status-chip asset-status-chip--${status.tone}`}>{status.label}</span>
          {asset.duration ? <span>{copy.duration(asset.duration)}</span> : null}
        </div>
        <button className="asset-inspect-button" onClick={() => editing.actions.rename(asset.id)}>{t.common.rename}</button>
        <button className="asset-inspect-button" onClick={() => onInspect(asset.id)} aria-label={copy.inspectAria(asset.name)}>{copy.inspect} <span aria-hidden="true">↗</span></button>
      </div>
    </article>
  )
}

type AssetBrowserProps = {
  editing: AssetLibraryEditController
  title: string
  breadcrumb: string
  isRoot: boolean
  matchingAssetCount: number
  assets: AssetView[]
  filtered?: boolean
  onInspect: (id: string) => void
  analysis?: ReactNode
}

export function AssetBrowser({ title, breadcrumb, isRoot, matchingAssetCount, assets, filtered, onInspect, editing, analysis }: AssetBrowserProps) {
  const { t } = useI18n()
  const copy = t.assets
  return (
    <section className="asset-list-card">
      <header className="asset-list-card__head">
        <div>
          <strong>{title}</strong>
          <p>{breadcrumb}</p>
        </div>
        <small>{assets.length} / {matchingAssetCount}</small>
      </header>
      {analysis}
      {assets.length > 0 && <div className="asset-edit-toolbar"><button onClick={editing.actions.selectAll}>{editing.model.selectedIds.length === assets.length ? t.common.clearSelection : copy.selectAllInList}</button><span>{copy.selectedCount(editing.model.selectedIds.length)}</span><button disabled={!editing.model.selectedIds.length || editing.model.busy} onClick={editing.actions.remove}>{copy.removeFromLibrary}</button></div>}
      {assets.length > 0 ? (
        <div className="asset-list-card__body">
          {assets.map((asset) => <AssetCard key={asset.id} asset={asset} onInspect={onInspect} editing={editing} />)}
        </div>
      ) : (
        <div className="asset-list-card__empty">
          {filtered ? copy.emptyFiltered : matchingAssetCount === 0 && isRoot
            ? copy.emptyAll
            : copy.emptyDirect}
        </div>
      )}
    </section>
  )
}
