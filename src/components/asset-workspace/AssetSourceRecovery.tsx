// 显式素材来源健康检查与重链路确认界面；不会自动重分析或改写未确认路径。
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

  return (
    <>
      <section className="asset-health-card">
        <div>
          <strong>源文件健康</strong>
          <p>正常 {health?.online ?? 0} · 缺失 {health?.missing ?? 0} · 已变化 {health?.changed ?? 0} · 不可读 {health?.unreadable ?? 0} · 未检查 {health?.unchecked ?? 0}</p>
        </div>
        <div className="asset-health-card__actions">
          {health?.activeTaskId ? (
            <button className="outline-button" onClick={() => onCancelHealthScan(health.activeTaskId!)}>取消检查</button>
          ) : (
            <button className="primary-button" onClick={onStartHealthScan} disabled={!projectReady}>检查源文件</button>
          )}
          {issueCount > 0 && <button className="outline-button" onClick={onOpenRelink} disabled={!projectReady}>修复源文件位置</button>}
        </div>
      </section>

      {relinkPreview && hasRelinkSource && (
        <section className="asset-relink-card">
          <div>
            <strong>重链路预览</strong>
            <p>已选择新的素材根目录。原目录结构匹配成功后才会更新引用。</p>
          </div>
          <p>唯一匹配 {relinkPreview.matches.length} 个，未确认 {relinkPreview.unmatchedCount} 个。现有分析证据会保留。</p>
          <div className="asset-relink-card__actions">
            <button className="primary-button" onClick={onConfirmRelink}>确认重链路</button>
            <button className="outline-button" onClick={onCancelRelink}>取消</button>
          </div>
        </section>
      )}
    </>
  )
}
