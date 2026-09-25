// 素材首次分析展示口径：与 Rust progress 查询一致，音频无需画面识别。
import type { AssetAnalysisProgress, AssetAnalysisState, StoredAsset } from './local-store'
import { messages } from './i18n'

export const EMPTY_ANALYSIS_PROGRESS: AssetAnalysisProgress = { total: 0, ready: 0, analyzing: 0, queued: 0, failed: 0, readyVideo: 0, cancelled: 0 }
export const analysisStates: readonly AssetAnalysisState[] = ['ready', 'analyzing', 'queued', 'failed']

export function assetAnalysisState(asset: Pick<StoredAsset, 'analysisStatus' | 'visualAnalysisStatus' | 'kind' | 'analysisCancelled'>): AssetAnalysisState {
  if (asset.analysisCancelled) return 'queued'
  if (asset.analysisStatus !== 'ready') return asset.analysisStatus
  if (asset.kind !== 'video' && asset.kind !== 'image') return 'ready'
  if (asset.visualAnalysisStatus === 'ready') return 'ready'
  if (asset.visualAnalysisStatus === 'running') return 'analyzing'
  if (asset.visualAnalysisStatus === 'failed' || asset.visualAnalysisStatus === 'skipped') return 'failed'
  return 'queued'
}

export function analysisSummary(progress: AssetAnalysisProgress) {
  const copy = messages().analysis
  if (!progress.total) return copy.summaryEmpty
  if (analysisPendingCount(progress) > 0) return copy.summaryPending(progress.ready, progress.total, progress.analyzing, progress.queued - progress.cancelled, progress.failed)
  if (progress.cancelled) return copy.summaryCancelled(progress.ready, progress.cancelled, progress.failed)
  if (progress.failed) return copy.summaryFailed(progress.ready, progress.failed)
  return copy.summaryDone(progress.ready)
}

export function analysisPendingCount(progress: AssetAnalysisProgress) {
  return progress.analyzing + progress.queued - progress.cancelled
}

export type AnalysisAmbientStatus = 'idle' | 'analyzing' | 'attention'

export function analysisAmbientStatus(progress: AssetAnalysisProgress): AnalysisAmbientStatus {
  if (analysisPendingCount(progress) > 0) return 'analyzing'
  if (progress.failed > 0 || progress.cancelled > 0) return 'attention'
  return 'idle'
}
