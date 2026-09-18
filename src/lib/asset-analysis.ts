// 素材首次分析展示口径：与 Rust progress 查询一致，音频无需画面识别。
import type { AssetAnalysisProgress, AssetAnalysisState, StoredAsset } from './local-store'

export const EMPTY_ANALYSIS_PROGRESS: AssetAnalysisProgress = { total: 0, ready: 0, analyzing: 0, queued: 0, failed: 0, readyVideo: 0, cancelled: 0 }
export const analysisLabels = { ready: '已分析', analyzing: '分析中', queued: '未分析', failed: '分析失败' } as const

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
  if (!progress.total) return '导入素材后自动开始分析'
  if (analysisPendingCount(progress) > 0) return `正在分析素材 · 已分析 ${progress.ready}/${progress.total} · 分析中 ${progress.analyzing} · 等待 ${progress.queued - progress.cancelled}${progress.failed ? ` · 失败 ${progress.failed}` : ''}`
  if (progress.cancelled) return `${progress.ready} 个分析完成 · ${progress.cancelled} 个已取消${progress.failed ? ` · ${progress.failed} 个失败` : ''}`
  if (progress.failed) return `${progress.ready} 个分析完成，${progress.failed} 个失败`
  return `${progress.ready} 个素材分析完成`
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
