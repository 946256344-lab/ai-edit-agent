import type { AssetView } from './asset-workspace/AssetBrowser'

type AnalysisActivityProps = {
  analyzingCount: number
  queuedCount: number
  visibleAssets: AssetView[]
}

export function AnalysisActivity({ analyzingCount, queuedCount, visibleAssets }: AnalysisActivityProps) {
  if (analyzingCount === 0) return null
  const visibleAnalyzing = visibleAssets.filter((asset) => asset.status === 'analyzing').slice(0, 3)

  return (
    <aside className="analysis-activity" aria-live="polite">
      <header>
        <span className="state-dot working" />
        <span>正在分析媒体</span>
        <b>{analyzingCount}</b>
        {queuedCount > 0 && <p className="analysis-queue">另 {queuedCount} 个排队等待</p>}
      </header>
      {visibleAnalyzing.length > 0 && <ul>{visibleAnalyzing.map((asset) => <li key={asset.id}>{asset.name}</li>)}</ul>}
      {analyzingCount > visibleAnalyzing.length && <p>另有 {analyzingCount - visibleAnalyzing.length} 个任务正在运行</p>}
    </aside>
  )
}
