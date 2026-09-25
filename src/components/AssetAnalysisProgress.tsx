// 素材分析进度与筛选：按素材数展示四种互斥状态，不估算单文件百分比。
import type { AssetAnalysisProgress as Progress, AssetAnalysisState } from '../lib/local-store'
import { analysisPendingCount, analysisStates, analysisSummary } from '../lib/asset-analysis'
import { useI18n } from '../lib/i18n'
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
  placement?: 'card' | 'toolbar'
}

export function AssetAnalysisProgress({ progress, selected, onSelect, onRetry, retrying, notice, importing, onCancel, onResume, busy, placement = 'card' }: Props) {
  const copy = useI18n().t.analysis
  const states = analysisStates
  const pending = analysisPendingCount(progress) > 0
  const toolbar = placement === 'toolbar'
  const status = importing ? (toolbar ? copy.importingShort : copy.importingLong) : toolbar && pending ? copy.analyzedRatio(progress.ready, progress.total) : analysisSummary(progress)
  const actions = (
    <>
      {onCancel && pending && <button type="button" onClick={onCancel} disabled={busy}>{copy.cancel}</button>}
      {onResume && progress.cancelled > 0 && <button type="button" onClick={onResume} disabled={busy}>{copy.resume}</button>}
      {onRetry && progress.failed > 0 && <button type="button" onClick={onRetry} disabled={retrying || importing}>{retrying ? copy.retrying : copy.retryFailed}</button>}
    </>
  )
  const legend = (onSelect || !toolbar) && (
    <div className="asset-analysis__legend">
      {onSelect && <button type="button" aria-pressed={!selected} onClick={() => onSelect(null)}>{copy.all(progress.total)}</button>}
      {states.map((state) => onSelect
        ? <button key={state} type="button" aria-pressed={selected === state} onClick={() => onSelect(selected === state ? null : state)}><i className={`asset-analysis__dot asset-analysis__segment--${state}`} />{copy.labels[state]} {progress[state]}</button>
        : <span key={state}>{copy.labels[state]} {progress[state]}</span>)}
    </div>
  )
  const bar = progress.total > 0 && (
    <div className="asset-analysis__bar" role="progressbar" aria-label={copy.barLabel} aria-valuemin={0} aria-valuemax={progress.total} aria-valuenow={progress.ready} aria-valuetext={analysisSummary(progress)}>
      {states.map((state) => <span key={state} className={`asset-analysis__segment asset-analysis__segment--${state}`} style={{ width: `${progress[state] / progress.total * 100}%` }} />)}
    </div>
  )

  return (
    <section className={`asset-analysis asset-analysis--${placement}`} aria-label={copy.sectionLabel}>
      <div className="asset-analysis__heading">
        {toolbar ? legend : <span role="status">{status}</span>}
        <span className="asset-analysis__actions">
          {toolbar && (pending || importing) && <span role="status">{status}</span>}
          {actions}
        </span>
      </div>
      {bar}
      {!toolbar && legend}
      {notice && <p className="asset-analysis__notice" role="status">{notice}</p>}
    </section>
  )
}
