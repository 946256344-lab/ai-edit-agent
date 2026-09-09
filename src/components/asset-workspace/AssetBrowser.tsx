// 显示当前目录的直属素材卡片；默认只展示用户可读状态，不承担目录过滤或后台轮询。
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

function analysisStatusLabel(asset: AssetView) {
  if (asset.sourceHealthStatus === 'missing' || asset.sourceHealthStatus === 'unreadable') {
    return { tone: 'failed' as const, label: '无法读取' }
  }
  if (asset.sourceHealthStatus === 'changed') {
    return { tone: 'failed' as const, label: '文件已变化' }
  }
  if (asset.status === 'failed' || asset.visualStatus === 'failed') {
    return { tone: 'failed' as const, label: '无法读取' }
  }
  if (asset.status === 'ready' && (asset.visualStatus === 'ready' || asset.visualStatus === 'skipped')) {
    return { tone: 'ready' as const, label: '已就绪' }
  }
  return { tone: 'analyzing' as const, label: '分析中' }
}

function AssetCard({ asset }: { asset: AssetView }) {
  const status = analysisStatusLabel(asset)

  return (
    <article className="asset-card">
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
        <div className="asset-chip-row">
          <span className={`asset-status-chip asset-status-chip--${status.tone}`}>{status.label}</span>
          {asset.duration ? <span>时长 {asset.duration}</span> : null}
        </div>
      </div>
    </article>
  )
}

type AssetBrowserProps = {
  title: string
  breadcrumb: string
  matchingAssetCount: number
  assets: AssetView[]
}

export function AssetBrowser({ title, breadcrumb, matchingAssetCount, assets }: AssetBrowserProps) {
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
          {assets.map((asset) => <AssetCard key={asset.id} asset={asset} />)}
        </div>
      ) : (
        <div className="asset-list-card__empty">
          {matchingAssetCount === 0 && title === '全部素材'
            ? '把素材拖到这里，或使用上方「导入文件 / 导入文件夹」。'
            : '当前目录没有直属素材。'}
        </div>
      )}
    </section>
  )
}
