// 显式素材来源恢复界面：仅在发现缺失/变化或正在检查时展示，不改写未确认路径。
import type { AssetHealthScanSummary, AssetRelinkPreview } from '../../lib/local-store'
import { useI18n } from '../../lib/i18n'

type AssetSourceRecoveryProps = {
  projectReady: boolean
  health: AssetHealthScanSummary | null
  relinkPreview: AssetRelinkPreview | null
  hasRelinkSource: boolean
  onStartHealthScan: () => void
  onCancelHealthScan: (taskId: string) => void
  onOpenRelink: () => void
  onConfirmRelink: () => void
  onCancelRelink: () => void
}

export function AssetSourceRecovery({
  projectReady,
  health,
  relinkPreview,
  hasRelinkSource,
  onStartHealthScan,
  onCancelHealthScan,
  onOpenRelink,
  onConfirmRelink,
  onCancelRelink,
}: AssetSourceRecoveryProps) {
  const { t } = useI18n()
  const copy = t.recovery
  const issueCount = health ? health.missing + health.changed + health.unreadable : 0
  const scanning = Boolean(health?.activeTaskId)

  return (
    <>
      {(issueCount > 0 || scanning) && (
        <section className="asset-health-card">
          <div>
            <strong>
              {scanning
                ? copy.scanning
                : copy.issues(issueCount)}
            </strong>
            <p>
              {scanning
                ? copy.scanningHint
                : copy.relinkHint}
            </p>
          </div>
          <div className="asset-health-card__actions">
            {scanning ? (
              <button className="outline-button" onClick={() => onCancelHealthScan(health!.activeTaskId!)}>
                {copy.cancelScan}
              </button>
            ) : (
              <>
                <button className="primary-button" onClick={onOpenRelink} disabled={!projectReady}>
                  {copy.relink}
                </button>
                <button className="outline-button" onClick={onStartHealthScan} disabled={!projectReady}>
                  {copy.rescan}
                </button>
              </>
            )}
          </div>
        </section>
      )}

      {relinkPreview && hasRelinkSource && (
        <section className="asset-relink-card">
          <div>
            <strong>{copy.confirmTitle}</strong>
            <p>{copy.confirmHint}</p>
          </div>
          <p>{copy.matchSummary(relinkPreview.matches.length, relinkPreview.unmatchedCount)}</p>
          <div className="asset-relink-card__actions">
            <button className="primary-button" onClick={onConfirmRelink}>{copy.confirm}</button>
            <button className="outline-button" onClick={onCancelRelink}>{t.common.cancel}</button>
          </div>
        </section>
      )}
    </>
  )
}
