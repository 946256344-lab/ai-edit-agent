// 右下角素材分析活动提示，只投影 controller 已提供的队列计数和当前素材。
import type { AssetView } from './asset-workspace/AssetBrowser'

type AnalysisActivityProps = {
  onCancel: () => void
  busy: boolean
  analyzingCount: number
  queuedCount: number
  visibleAssets: AssetView[]
}

export function AnalysisActivity({ analyzingCount, queuedCount, visibleAssets, onCancel, busy }: AnalysisActivityProps) {
  if (analyzingCount + queuedCount === 0) return null
  const visibleAnalyzing = visibleAssets.filter((asset) => asset.analysisState === 'analyzing').slice(0, 3)

  return (
    <aside className="analysis-activity" aria-live="polite">
      <header>
        <span className="state-dot working" />
        <span>{analyzingCount > 0 ? '正在分析媒体' : '等待素材分析'}</span>
        <b>{analyzingCount || queuedCount}</b>
        {analyzingCount > 0 && queuedCount > 0 && <p className="analysis-queue">另 {queuedCount} 个排队等待</p>}
        <button onClick={onCancel} disabled={busy}>取消分析</button>
      </header>
      {visibleAnalyzing.length > 0 && <ul>{visibleAnalyzing.map((asset) => <li key={asset.id}>{asset.name}</li>)}</ul>}
      {analyzingCount > visibleAnalyzing.length && <p>另有 {analyzingCount - visibleAnalyzing.length} 个任务正在运行</p>}
    </aside>
  )
}
