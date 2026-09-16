// 素材分析进度与筛选：按素材数展示四种互斥状态，不估算单文件百分比。
import type { AssetAnalysisProgress as Progress, AssetAnalysisState } from '../lib/local-store'
import { analysisLabels, analysisPendingCount, analysisSummary } from '../lib/asset-analysis'
import './asset-analysis.css'

type Props = {
  progress: Progress
  selected?: AssetAnalysisState | null
  onSelect?: (state: AssetAnalysisState | null) => void
  onCancel?: () => void
  onResume?: () => void
  busy?: boolean
  onRetry?: () => void
  retrying: boolean
  notice: string | null
  importing?: boolean
  compact?: boolean
}

export function AssetAnalysisProgress({ progress, selected, onSelect, onRetry, retrying, notice, importing, compact, onCancel, onResume, busy }: Props) {
  const states = Object.keys(analysisLabels) as AssetAnalysisState[]
  return (
    <section className={`asset-analysis${compact ? ' asset-analysis--compact' : ''}`} aria-label="素材分析进度">
      <div className="asset-analysis__heading">
        <span role="status">{importing ? '正在导入素材，导入后自动开始分析…' : analysisSummary(progress)}</span>
        {onCancel && analysisPendingCount(progress) > 0 && <button type="button" onClick={onCancel} disabled={busy}>取消分析</button>}
        {onResume && progress.cancelled > 0 && <button type="button" onClick={onResume} disabled={busy}>继续分析</button>}
        {onRetry && progress.failed > 0 && <button type="button" onClick={onRetry} disabled={retrying || importing}>{retrying ? '正在提交重试…' : '重试失败素材'}</button>}
      </div>
      {progress.total > 0 && <div className="asset-analysis__bar" role="progressbar" aria-label="已分析素材" aria-valuemin={0} aria-valuemax={progress.total} aria-valuenow={progress.ready} aria-valuetext={analysisSummary(progress)}>
        {states.map(state => <span key={state} className={`asset-analysis__segment asset-analysis__segment--${state}`} style={{ width: `${progress[state] / progress.total * 100}%` }} />)}
      </div>}
      {!compact && <div className="asset-analysis__legend">
        {onSelect && <button type="button" aria-pressed={!selected} onClick={() => onSelect(null)}>全部 {progress.total}</button>}
        {states.map(state => onSelect
          ? <button key={state} type="button" aria-pressed={selected === state} onClick={() => onSelect(selected === state ? null : state)}><i className={`asset-analysis__dot asset-analysis__segment--${state}`} />{analysisLabels[state]} {progress[state]}</button>
          : <span key={state}>{analysisLabels[state]} {progress[state]}</span>)}
      </div>}
      {notice && <p className="asset-analysis__notice" role="status">{notice}</p>}
    </section>
  )
}
