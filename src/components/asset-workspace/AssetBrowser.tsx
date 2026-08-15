// 显示当前目录的直属素材卡片和分页动作，不负责目录过滤或后台轮询。
import type { StoredAsset } from '../../lib/local-store'

export type AssetView = {
  id: string
  name: string
  folderName: string | null
  relativePath: string | null
  kind: 'video' | 'image' | 'audio' | 'other'
  duration: string
  status: 'ready' | 'analyzing' | 'queued' | 'failed'
  visualStatus: 'queued' | 'running' | 'ready' | 'failed' | 'skipped'
  sourceHealthStatus: StoredAsset['sourceHealthStatus']
  thumbnailUrl: string | null
}

function visualStatusLabel(status: AssetView['visualStatus']) {
  switch (status) {
    case 'ready': return '视觉完成'
    case 'running': return '视觉分析中'
    case 'queued': return '视觉排队'
    case 'failed': return '视觉失败'
    case 'skipped': return '视觉跳过'
  }
}

function sourceHealthLabel(status: AssetView['sourceHealthStatus']) {
  switch (status) {
    case 'missing': return '源文件缺失'
    case 'changed': return '源文件已变化'
    case 'unreadable': return '源文件不可读'
    default: return null
  }
}

function AssetCard({ asset, onInspect }: { asset: AssetView; onInspect: (assetId: string) => void }) {
  const facts = [
    asset.status === 'ready' ? '技术分析完成' : asset.status === 'failed' ? '技术失败' : asset.status === 'queued' ? '等待分析' : '分析中',
    visualStatusLabel(asset.visualStatus),
    sourceHealthLabel(asset.sourceHealthStatus),
    asset.kind === 'other' ? '其他素材' : asset.kind,
    asset.duration ? `时长 ${asset.duration}` : null,
  ].filter((fact): fact is string => Boolean(fact))

  return (
    <button type="button" className="asset-card" onClick={() => onInspect(asset.id)}>
      <div className={`asset-card-thumb asset-card-thumb-${asset.kind}`}>
        {asset.thumbnailUrl && <img src={asset.thumbnailUrl} alt="" />}
        <span>{asset.kind === 'video' ? 'VIDEO' : asset.kind.toUpperCase()}</span>
        {asset.duration && <time>{asset.duration}</time>}
      </div>
      <div className="asset-card-body">
        <header>
          <strong>{asset.name}</strong>
          <small>{asset.relativePath ?? asset.folderName ?? '未归类素材'}</small>
        </header>
        <div className="asset-chip-row">{facts.map((fact) => <span key={fact}>{fact}</span>)}</div>
      </div>
    </button>
  )
}

type AssetBrowserProps = {
  title: string
  breadcrumb: string
  matchingAssetCount: number
  assets: AssetView[]
  onInspect: (assetId: string) => void
}

export function AssetBrowser({ title, breadcrumb, matchingAssetCount, assets, onInspect }: AssetBrowserProps) {
  return (
    <section className="asset-list-card">
      <header className="asset-list-card__head">
        <div>
          <strong>{title}</strong>
          <p>{breadcrumb}</p>
        </div>
        <small>{assets.length} / {matchingAssetCount}</small>
      </header>
      {assets.length > 0 ? (
        <div className="asset-list-card__body">
          {assets.map((asset) => <AssetCard key={asset.id} asset={asset} onInspect={onInspect} />)}
        </div>
      ) : (
        <div className="asset-list-card__empty">当前目录没有直属素材。</div>
      )}
    </section>
  )
}
