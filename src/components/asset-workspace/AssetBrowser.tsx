// 显示当前目录的直属素材卡片；默认只展示用户可读状态，不承担目录过滤或后台轮询。
import type { AssetLibraryEditController } from '../../hooks/useAssetLibraryEditController'
import type { AssetAnalysisState, StoredAsset } from '../../lib/local-store'
import type { ReactNode } from 'react'
import { analysisLabels } from '../../lib/asset-analysis'

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

function analysisStatusLabel(asset: AssetView) {
  if (asset.sourceHealthStatus === 'missing' || asset.sourceHealthStatus === 'unreadable') {
    return { tone: 'failed' as const, label: '无法读取' }
  }
  if (asset.sourceHealthStatus === 'changed') {
    return { tone: 'failed' as const, label: '文件已变化' }
  }
  return { tone: asset.analysisState, label: asset.analysisCancelled ? '已取消分析' : analysisLabels[asset.analysisState] }
}

function AssetCard({ asset, onInspect, editing }: { asset: AssetView; onInspect: (id: string) => void; editing: AssetLibraryEditController }) {
  const status = analysisStatusLabel(asset)

  return (
    <article className="asset-card">
      <label className="asset-select"><input type="checkbox" aria-label={`选择 ${asset.name}`} checked={editing.model.selectedIds.includes(asset.id)} onChange={() => editing.actions.toggle(asset.id)} disabled={editing.model.busy} />选择</label>
      <div className={`asset-card-thumb asset-card-thumb-${asset.kind}`}>
        {asset.thumbnailUrl && <img src={asset.thumbnailUrl} alt="" loading="lazy" decoding="async" />}
        <span>{asset.kind === 'video' ? 'VIDEO' : asset.kind.toUpperCase()}</span>
        {asset.duration && <time>{asset.duration}</time>}
      </div>
      <div className="asset-card-body">
        <header>
          <strong title={asset.name}>{asset.name}</strong>
          <small>{asset.relativePath ?? asset.folderName ?? '未归类素材'}</small>
        </header>
        <div className="asset-chip-row">
          <span className={`asset-status-chip asset-status-chip--${status.tone}`}>{status.label}</span>
          {asset.duration ? <span>时长 {asset.duration}</span> : null}
        </div>
        <button className="asset-inspect-button" onClick={() => editing.actions.rename(asset.id)}>重命名</button>
        <button className="asset-inspect-button" onClick={() => onInspect(asset.id)} aria-label={`查看 ${asset.name} 的分析结果`}>查看分析 <span aria-hidden="true">↗</span></button>
      </div>
    </article>
  )
}

type AssetBrowserProps = {
  editing: AssetLibraryEditController
  title: string
  breadcrumb: string
  matchingAssetCount: number
  assets: AssetView[]
  filtered?: boolean
  onInspect: (id: string) => void
  analysis?: ReactNode
}

export function AssetBrowser({ title, breadcrumb, matchingAssetCount, assets, filtered, onInspect, editing, analysis }: AssetBrowserProps) {
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
      {assets.length > 0 && <div className="asset-edit-toolbar"><button onClick={editing.actions.selectAll}>{editing.model.selectedIds.length === assets.length ? '取消全选' : '全选当前列表'}</button><span>已选 {editing.model.selectedIds.length} 项</span><button disabled={!editing.model.selectedIds.length || editing.model.busy} onClick={editing.actions.remove}>移出素材库</button></div>}
      {assets.length > 0 ? (
        <div className="asset-list-card__body">
          {assets.map((asset) => <AssetCard key={asset.id} asset={asset} onInspect={onInspect} editing={editing} />)}
        </div>
      ) : (
        <div className="asset-list-card__empty">
          {filtered ? '当前目录没有符合此分析状态的素材。' : matchingAssetCount === 0 && title === '全部素材'
            ? '还没有素材。点击上方「导入文件」或「导入文件夹」，开始准备你的第一条视频。'
            : '当前目录没有直属素材。'}
        </div>
      )}
    </section>
  )
}
