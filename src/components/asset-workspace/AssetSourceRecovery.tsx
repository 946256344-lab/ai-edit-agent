// 显式素材来源恢复界面：仅在发现缺失/变化或正在检查时展示，不改写未确认路径。
import type { AssetHealthScanSummary, AssetRelinkPreview } from '../../lib/local-store'

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
  const issueCount = health ? health.missing + health.changed + health.unreadable : 0
  const scanning = Boolean(health?.activeTaskId)

  return (
    <>
      {(issueCount > 0 || scanning) && (
        <section className="asset-health-card">
          <div>
            <strong>
              {scanning
                ? '正在检查素材文件'
                : `发现 ${issueCount} 个素材文件已移动或无法读取`}
            </strong>
            <p>
              {scanning
                ? '检查完成后，如有缺失会提示重新定位。'
                : '重新定位后会保留已有分析结果。'}
            </p>
          </div>
          <div className="asset-health-card__actions">
            {scanning ? (
              <button className="outline-button" onClick={() => onCancelHealthScan(health!.activeTaskId!)}>
                取消检查
              </button>
            ) : (
              <>
                <button className="primary-button" onClick={onOpenRelink} disabled={!projectReady}>
                  重新定位素材
                </button>
                <button className="outline-button" onClick={onStartHealthScan} disabled={!projectReady}>
                  再检查一次
                </button>
              </>
            )}
          </div>
        </section>
      )}

      {relinkPreview && hasRelinkSource && (
        <section className="asset-relink-card">
          <div>
            <strong>确认新的素材位置</strong>
            <p>匹配成功后才会更新引用，已有分析会保留。</p>
          </div>
          <p>唯一匹配 {relinkPreview.matches.length} 个，未确认 {relinkPreview.unmatchedCount} 个。</p>
          <div className="asset-relink-card__actions">
            <button className="primary-button" onClick={onConfirmRelink}>确认重新定位</button>
            <button className="outline-button" onClick={onCancelRelink}>取消</button>
          </div>
        </section>
      )}
    </>
  )
}
