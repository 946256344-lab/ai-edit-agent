import { convertFileSrc } from '@tauri-apps/api/core'
import type { AssetCollection, AssetEvidence, AssetHealthScanSummary, AssetPage, AssetRelinkPreview, AssetTaskCenter, StoredAsset } from '../lib/local-store'

type AssetView = {
  id: string
  name: string
  folderName: string | null
  relativePath: string | null
  kind: 'video' | 'image' | 'audio' | 'other'
  duration: string
  status: 'ready' | 'analyzing' | 'queued' | 'failed'
  visualStatus: 'queued' | 'running' | 'ready' | 'failed' | 'skipped'
  favorite: boolean
  rating: number
  note: string
  excluded: boolean
  userTags: string[]
  collectionIds: string[]
  sourceHealthStatus: StoredAsset['sourceHealthStatus']
  tags: string[]
  color: string
  thumbnailUrl: string | null
}

type AssetTreeNode = {
  name: string
  path: string
  assetCount: number
  children: AssetTreeNode[]
}

type AssetManagementPanelProps = {
  activeProjectId: string | null
  storeReady: boolean
  assetPage: Pick<AssetPage, 'total' | 'counts'>
  assetTree: { root: AssetTreeNode; unfiledCount: number }
  activeFolderNode: AssetTreeNode
  folderBreadcrumb: string[]
  visibleAssetFolders: AssetTreeNode[]
  filteredAssets: AssetView[]
  selectedAssetIds: Set<string>
  setSelectedAssetIds: (value: Set<string>) => void
  assetSearch: string
  setAssetSearch: (value: string) => void
  assetKindFilter: 'all' | AssetView['kind']
  setAssetKindFilter: (value: 'all' | AssetView['kind']) => void
  assetStatusFilter: 'all' | AssetView['status']
  setAssetStatusFilter: (value: 'all' | AssetView['status']) => void
  assetVisualFilter: 'all' | 'storyboard-ready' | AssetView['visualStatus']
  setAssetVisualFilter: (value: 'all' | 'storyboard-ready' | AssetView['visualStatus']) => void
  assetFolderFilter: string
  setAssetFolderFilter: (value: string) => void
  assetUserFilter: 'all' | 'favorite' | 'excluded' | 'available'
  setAssetUserFilter: (value: 'all' | 'favorite' | 'excluded' | 'available') => void
  assetCollectionFilter: string
  setAssetCollectionFilter: (value: string) => void
  assetCollections: AssetCollection[]
  assetBatchNotice: string | null
  isRunningAssetBatch: boolean
  onClearSelection: () => void
  onRetrySelectedAssetAnalysis: () => void
  onSkipSelectedVisualAnalysis: () => void
  onApplyUserMetadata: (fields: { favorite?: boolean; rating?: number; note?: string; excluded?: boolean }, successMessage: string) => void
  onAddTagToSelected: () => void
  onAddSelectedToCollection: () => void
  onSelectFolder: (path: string) => void
  onSelectAssetEvidence: (assetId: string) => void
  onImportAssets: () => void
  onImportAssetFolder: () => void
  onStartAssetHealthScan: () => void
  onCancelAssetHealthScan: (taskId: string) => void
  onOpenRelink: () => void
  onConfirmRelink: () => void
  onCancelRelink: () => void
  assetHealth: AssetHealthScanSummary | null
  assetTaskCenter: AssetTaskCenter | null
  assetTaskCenterOpen: boolean
  setAssetTaskCenterOpen: (value: boolean | ((current: boolean) => boolean)) => void
  assetRelinkPreview: AssetRelinkPreview | null
  assetRelinkSourceDirectory: string | null
  assetRelinkPreserveAnalysis: boolean
  setAssetRelinkPreserveAnalysis: (value: boolean) => void
  setAssetEvidenceNull: () => void
  assetEvidence: AssetEvidence | null
}

function formatTimeMs(timeMs: number | null) {
  if (timeMs === null) return '图片'
  const seconds = Math.max(0, Math.floor(timeMs / 1000))
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(seconds % 60).padStart(2, '0')}`
}

function visualStatusLabel(status: AssetView['visualStatus']) {
  switch (status) {
    case 'ready': return '视觉完成'
    case 'running': return '视觉分析中'
    case 'queued': return '视觉排队'
    case 'failed': return '视觉失败'
    case 'skipped': return '视觉跳过'
    default: return '视觉未开始'
  }
}

function healthLabel(status: StoredAsset['sourceHealthStatus']) {
  switch (status) {
    case 'online': return '正常'
    case 'missing': return '缺失'
    case 'changed': return '已变化'
    case 'unreadable': return '不可读'
    default: return '未检查'
  }
}

function AssetFolderTree({ node, activePath, onSelectFolder, depth = 0 }: {
  node: AssetTreeNode
  activePath: string
  onSelectFolder: (path: string) => void
  depth?: number
}) {
  const isActive = node.path === activePath
  return (
    <div className="asset-tree-node" data-depth={depth}>
      <button type="button" className={`asset-tree-row ${isActive ? 'is-active' : ''}`} onClick={() => onSelectFolder(node.path)}>
        <span className="asset-tree-caret">{node.children.length > 0 ? '▸' : '•'}</span>
        <span className="asset-tree-name">{node.name}</span>
        <small>{node.assetCount}</small>
      </button>
      {node.children.length > 0 && (
        <div className="asset-tree-children">
          {node.children.map((child) => (
            <AssetFolderTree key={child.path} node={child} activePath={activePath} onSelectFolder={onSelectFolder} depth={depth + 1} />
          ))}
        </div>
      )}
    </div>
  )
}

function AssetCard({ asset, selected, onSelectAsset }: { asset: AssetView; selected: boolean; onSelectAsset: (assetId: string) => void }) {
  const userJudgments = [
    asset.favorite ? '收藏' : '未收藏',
    asset.excluded ? '禁止使用' : '允许使用',
    asset.rating ? `${asset.rating} 星` : '未评分',
    ...asset.userTags.slice(0, 2),
  ].filter(Boolean)
  const evidence = [
    asset.status === 'ready' ? '技术分析完成' : asset.status === 'failed' ? '技术失败' : asset.status === 'queued' ? '等待分析' : '分析中',
    visualStatusLabel(asset.visualStatus),
    asset.sourceHealthStatus !== 'online' && asset.sourceHealthStatus !== 'unchecked' ? healthLabel(asset.sourceHealthStatus) : null,
    asset.kind === 'other' ? '其他素材' : asset.kind,
    asset.duration ? `时长 ${asset.duration}` : null,
  ].filter(Boolean) as string[]

  return (
    <button type="button" className={`asset-card ${selected ? 'is-selected' : ''}`} onClick={() => onSelectAsset(asset.id)}>
      <div className={`asset-card-thumb asset-card-thumb-${asset.color}`}>
        {asset.thumbnailUrl && <img src={asset.thumbnailUrl} alt="" />}
        <span>{asset.kind === 'video' ? 'VIDEO' : asset.kind.toUpperCase()}</span>
        {asset.duration && <time>{asset.duration}</time>}
      </div>
      <div className="asset-card-body">
        <header>
          <strong>{asset.name}</strong>
          <small>{asset.relativePath ?? asset.folderName ?? '未归类素材'}</small>
        </header>
        <div className="asset-chip-row">{userJudgments.map((item) => <span key={item}>{item}</span>)}</div>
        <div className="asset-chip-row asset-chip-row-muted">{evidence.map((item) => <span key={item}>{item}</span>)}</div>
      </div>
    </button>
  )
}

export function AssetManagementPanel({
  activeProjectId,
  storeReady,
  assetPage,
  assetTree,
  activeFolderNode,
  folderBreadcrumb,
  visibleAssetFolders,
  filteredAssets,
  selectedAssetIds,
  setSelectedAssetIds,
  assetSearch,
  setAssetSearch,
  assetKindFilter,
  setAssetKindFilter,
  assetStatusFilter,
  setAssetStatusFilter,
  assetVisualFilter,
  setAssetVisualFilter,
  assetFolderFilter,
  setAssetFolderFilter,
  assetUserFilter,
  setAssetUserFilter,
  assetCollectionFilter,
  setAssetCollectionFilter,
  assetCollections,
  assetBatchNotice,
  isRunningAssetBatch,
  onClearSelection,
  onRetrySelectedAssetAnalysis,
  onSkipSelectedVisualAnalysis,
  onApplyUserMetadata,
  onAddTagToSelected,
  onAddSelectedToCollection,
  onSelectFolder,
  onSelectAssetEvidence,
  onImportAssets,
  onImportAssetFolder,
  onStartAssetHealthScan,
  onCancelAssetHealthScan,
  onOpenRelink,
  onConfirmRelink,
  onCancelRelink,
  assetHealth,
  assetTaskCenter,
  assetTaskCenterOpen,
  setAssetTaskCenterOpen,
  assetRelinkPreview,
  assetRelinkSourceDirectory,
  assetRelinkPreserveAnalysis,
  setAssetRelinkPreserveAnalysis,
  setAssetEvidenceNull,
  assetEvidence,
}: AssetManagementPanelProps) {
  const hasFilters = assetSearch.trim() !== '' || assetKindFilter !== 'all' || assetStatusFilter !== 'all' || assetVisualFilter !== 'all' || assetFolderFilter !== 'all' || assetUserFilter !== 'all' || assetCollectionFilter !== 'all'
  const allVisibleSelected = filteredAssets.length > 0 && filteredAssets.every((asset) => selectedAssetIds.has(asset.id))
  const healthIssues = assetHealth ? assetHealth.missing + assetHealth.changed + assetHealth.unreadable : 0
  const currentTitle = assetFolderFilter === 'all'
    ? '全部素材'
    : assetFolderFilter === '__unfiled__'
      ? '未归类素材'
      : folderBreadcrumb[folderBreadcrumb.length - 1] ?? '素材目录'
  const relinkCount = assetRelinkPreview?.matches.length ?? 0

  return (
    <section className="asset-workbench">
      <header className="asset-workbench__header">
        <div>
          <span className="panel-kicker">素材管理</span>
          <strong>{assetPage.counts.total} 个素材</strong>
          <p>{activeProjectId ? '所有素材只做引用管理，用户判断与分析证据分层保存。' : '请选择项目后开始管理素材。'}</p>
        </div>
        <div className="asset-workbench__actions">
          <button className="import-button" onClick={onImportAssets} disabled={!activeProjectId || !storeReady}>导入文件</button>
          <button className="import-button" onClick={onImportAssetFolder} disabled={!activeProjectId || !storeReady}>导入文件夹</button>
        </div>
      </header>

      <div className="asset-workbench__grid">
        <section className="asset-workbench__left">
          <section className="asset-metrics">
            <article><b>{assetPage.counts.ready}</b><span>已分析</span></article>
            <article><b>{assetPage.counts.analyzing}</b><span>分析中</span></article>
            <article><b>{assetPage.counts.queued}</b><span>排队中</span></article>
            <article><b>{assetPage.counts.failed}</b><span>失败</span></article>
          </section>

          <section className="asset-search-card">
            <label className="asset-search-field">
              <span>搜索素材</span>
              <input value={assetSearch} onChange={(event) => setAssetSearch(event.target.value)} placeholder="名称、文件夹或路径" />
            </label>
            <div className="asset-filter-tabs" aria-label="素材类型筛选">
              {([['all', '全部'], ['video', '视频'], ['image', '图片'], ['audio', '音频']] as const).map(([value, label]) => (
                <button key={value} className={assetKindFilter === value ? 'active' : ''} onClick={() => setAssetKindFilter(value)}>{label}</button>
              ))}
            </div>
            <div className="asset-filter-grid">
              <label><span>技术状态</span><select value={assetStatusFilter} onChange={(event) => setAssetStatusFilter(event.target.value as typeof assetStatusFilter)}><option value="all">全部状态</option><option value="ready">已分析</option><option value="analyzing">分析中</option><option value="queued">排队中</option><option value="failed">失败</option></select></label>
              <label><span>视觉状态</span><select value={assetVisualFilter} onChange={(event) => setAssetVisualFilter(event.target.value as typeof assetVisualFilter)}><option value="all">全部视觉状态</option><option value="storyboard-ready">可用于 storyboard</option><option value="ready">视觉完成</option><option value="running">视觉分析中</option><option value="queued">视觉排队中</option><option value="failed">视觉失败</option><option value="skipped">视觉跳过</option></select></label>
              <label><span>素材文件夹</span><select value={assetFolderFilter} onChange={(event) => onSelectFolder(event.target.value)}><option value="all">全部文件夹</option><option value="__unfiled__">未归类素材</option>{assetTree.root.children.map((node) => <option key={node.path} value={node.path}>{node.name}</option>)}</select></label>
              <label><span>用户状态</span><select value={assetUserFilter} onChange={(event) => setAssetUserFilter(event.target.value as typeof assetUserFilter)}><option value="all">全部用户状态</option><option value="favorite">已收藏</option><option value="available">允许使用</option><option value="excluded">禁止使用</option></select></label>
              <label><span>素材集合</span><select value={assetCollectionFilter} onChange={(event) => setAssetCollectionFilter(event.target.value)}><option value="all">全部集合</option>{assetCollections.map((collection) => <option key={collection.id} value={collection.id}>{collection.name}（{collection.assetCount}）</option>)}</select></label>
            </div>
            <div className="asset-filter-summary">
              <span>已加载 {filteredAssets.length} / 匹配 {assetPage.total}</span>
              {hasFilters && <button onClick={() => { setAssetSearch(''); setAssetKindFilter('all'); setAssetStatusFilter('all'); setAssetVisualFilter('all'); setAssetFolderFilter('all'); setAssetUserFilter('all'); setAssetCollectionFilter('all') }}>清空筛选</button>}
            </div>
          </section>

          <section className="asset-breadcrumb-card">
            <div>
              <strong>{currentTitle}</strong>
              <p>{folderBreadcrumb.join(' / ')}</p>
            </div>
            <small>{assetFolderFilter === 'all' ? `${assetTree.root.assetCount} 个目录分支` : assetFolderFilter === '__unfiled__' ? `${assetTree.unfiledCount} 个未归类素材` : `${activeFolderNode.assetCount} 个直属素材`}</small>
          </section>

          <section className="asset-tree-card">
            <div className="asset-tree-card__head">
              <strong>目录树</strong>
              <small>只展示层级结构，不混入分析结果</small>
            </div>
            <div className="asset-tree-card__body">
              {assetTree.root.children.length > 0 ? assetTree.root.children.map((node) => <AssetFolderTree key={node.path} node={node} activePath={assetFolderFilter} onSelectFolder={onSelectFolder} />) : <p className="asset-empty-hint">导入文件夹后，这里会按层级显示目录。</p>}
            </div>
            {visibleAssetFolders.length > 0 && (
              <div className="asset-folder-strip">
                {visibleAssetFolders.slice(0, 4).map((folder) => (
                  <button key={folder.path} className="asset-folder-pill" onClick={() => { setAssetEvidenceNull(); onSelectFolder(folder.path) }}>
                    <span>{folder.name}</span>
                    <small>{folder.assetCount}</small>
                  </button>
                ))}
              </div>
            )}
          </section>
        </section>

        <section className="asset-workbench__center">
          <section className="asset-selection-card">
            <label>
              <input type="checkbox" checked={allVisibleSelected} onChange={(event) => setSelectedAssetIds(event.target.checked ? new Set(filteredAssets.slice(0, 200).map((asset) => asset.id)) : new Set())} />
              选择已加载素材
            </label>
            <span>{selectedAssetIds.size} 个已选择</span>
          </section>

          {selectedAssetIds.size > 0 && (
            <section className="asset-batch-card">
              <div className="asset-batch-card__actions">
                <button disabled={isRunningAssetBatch} onClick={() => onApplyUserMetadata({ favorite: true }, '已收藏所选素材。')}>收藏</button>
                <button disabled={isRunningAssetBatch} onClick={() => onApplyUserMetadata({ favorite: false }, '已取消所选素材的收藏。')}>取消收藏</button>
                <button disabled={isRunningAssetBatch} onClick={onAddTagToSelected}>添加标签</button>
                <button disabled={isRunningAssetBatch} onClick={onAddSelectedToCollection}>加入集合</button>
                <button disabled={isRunningAssetBatch} onClick={() => onApplyUserMetadata({ excluded: true }, '已将所选素材标记为禁止使用。')}>禁止使用</button>
                <button disabled={isRunningAssetBatch} onClick={() => onApplyUserMetadata({ excluded: false }, '已恢复所选素材为允许使用。')}>恢复使用</button>
                <button disabled={isRunningAssetBatch} onClick={() => { const rating = Number(window.prompt('输入评分 0–5')); if (Number.isInteger(rating) && rating >= 0 && rating <= 5) onApplyUserMetadata({ rating }, `已将所选素材评分设为 ${rating}。`) }}>评分</button>
                <button disabled={isRunningAssetBatch} onClick={() => { const note = window.prompt('输入批量备注（会覆盖所选素材的原备注）'); if (note !== null) onApplyUserMetadata({ note }, '已更新所选素材备注。') }}>备注</button>
                <button disabled={isRunningAssetBatch} onClick={onRetrySelectedAssetAnalysis}>重试技术分析</button>
                <button disabled={isRunningAssetBatch} onClick={onSkipSelectedVisualAnalysis}>跳过视觉分析</button>
                <button disabled={isRunningAssetBatch} onClick={onClearSelection}>取消选择</button>
              </div>
              {assetBatchNotice && <p>{assetBatchNotice}</p>}
            </section>
          )}

          <section className="asset-list-card">
            <div className="asset-list-card__head">
              <strong>{currentTitle}</strong>
              <small>{filteredAssets.length} / {assetPage.total}</small>
            </div>
            {assetFolderFilter !== 'all' && filteredAssets.length > 0 ? (
              <div className="asset-list-card__body">
                {filteredAssets.map((asset) => <AssetCard key={asset.id} asset={asset} selected={selectedAssetIds.has(asset.id)} onSelectAsset={onSelectAssetEvidence} />)}
              </div>
            ) : (
              <div className="asset-list-card__empty">{assetFolderFilter === 'all' ? '选择目录树中的文件夹，或者继续导入更多素材。' : '这个文件夹里暂无可显示的素材。'}</div>
            )}
          </section>
        </section>

        <aside className="asset-workbench__right">
          <section className="asset-health-card">
            <div>
              <strong>源文件健康</strong>
              <p>正常 {assetHealth?.online ?? 0} · 缺失 {assetHealth?.missing ?? 0} · 已变化 {assetHealth?.changed ?? 0} · 不可读 {assetHealth?.unreadable ?? 0} · 未检查 {assetHealth?.unchecked ?? 0}</p>
            </div>
            <div className="asset-health-card__actions">
              {assetHealth?.activeTaskId ? (
                <button className="outline-button" onClick={() => onCancelAssetHealthScan(assetHealth.activeTaskId!)}>取消检查</button>
              ) : (
                <button className="primary-button" onClick={onStartAssetHealthScan} disabled={!activeProjectId || !storeReady}>检查源文件</button>
              )}
              {healthIssues > 0 && <button className="outline-button" onClick={onOpenRelink} disabled={!activeProjectId || !storeReady}>修复源文件位置</button>}
            </div>
          </section>

          {assetRelinkPreview && assetRelinkSourceDirectory && (
            <section className="asset-relink-card">
              <div>
                <strong>重链路预览</strong>
                <p>{assetRelinkSourceDirectory}</p>
              </div>
              <p>唯一匹配 {relinkCount} 个，未确认 {assetRelinkPreview.unmatchedCount} 个。</p>
              <label className="asset-relink-card__check">
                <input type="checkbox" checked={assetRelinkPreserveAnalysis} onChange={(event) => setAssetRelinkPreserveAnalysis(event.target.checked)} />
                保留现有分析证据
              </label>
              <div className="asset-relink-card__actions">
                <button className="primary-button" onClick={onConfirmRelink}>确认重链路</button>
                <button className="outline-button" onClick={onCancelRelink}>取消</button>
              </div>
            </section>
          )}

          <section className="asset-task-card">
            <button className="asset-task-card__toggle" onClick={() => setAssetTaskCenterOpen((open) => !open)} aria-expanded={assetTaskCenterOpen}>
              <span>素材任务中心</span>
              <b>{(assetTaskCenter?.technical.queued ?? 0) + (assetTaskCenter?.technical.running ?? 0) + (assetTaskCenter?.visual.queued ?? 0) + (assetTaskCenter?.visual.running ?? 0)} 个活动任务</b>
            </button>
            {assetTaskCenterOpen && assetTaskCenter && (
              <div className="asset-task-card__body">
                <div className="asset-task-stage"><strong>技术分析</strong><span>运行 {assetTaskCenter.technical.running}</span><span>排队 {assetTaskCenter.technical.queued}</span><span className={assetTaskCenter.technical.failed ? 'warning' : ''}>失败 {assetTaskCenter.technical.failed}</span></div>
                <div className="asset-task-stage"><strong>视觉分析</strong><span>运行 {assetTaskCenter.visual.running}</span><span>排队 {assetTaskCenter.visual.queued}</span><span className={assetTaskCenter.visual.failed ? 'warning' : ''}>失败 {assetTaskCenter.visual.failed}</span><span>跳过 {assetTaskCenter.visual.skipped}</span></div>
                {assetTaskCenter.recentFailures.length > 0 && (
                  <div className="asset-task-failures">
                    <strong>最近失败</strong>
                    {assetTaskCenter.recentFailures.slice(0, 5).map((failure) => (
                      <button key={`${failure.stage}-${failure.assetId}`} onClick={() => { setSelectedAssetIds(new Set([failure.assetId])); setAssetStatusFilter(failure.stage === 'technical' ? 'failed' : 'all'); setAssetVisualFilter(failure.stage === 'visual' ? 'failed' : 'all') }}>
                        <span>{failure.displayName}</span>
                        <small>{failure.stage === 'technical' ? '技术分析失败' : '视觉分析失败'}</small>
                      </button>
                    ))}
                    {assetTaskCenter.recentFailures.some((failure) => failure.stage === 'technical') && <button className="retry-all-failed" disabled={isRunningAssetBatch} onClick={onRetrySelectedAssetAnalysis}>重试最近技术失败</button>}
                  </div>
                )}
              </div>
            )}
          </section>

          {assetEvidence ? (
            <section className="asset-evidence-card">
              <div className="asset-evidence-card__head">
                <div>
                  <span className="panel-kicker">画面证据</span>
                  <strong>{assetEvidence.displayName}</strong>
                </div>
                <button className="close-button" onClick={setAssetEvidenceNull} aria-label="关闭">x</button>
              </div>
              <p className="asset-evidence-card__note">{assetEvidence.visualAnalysisNote ?? (assetEvidence.visualAnalysisStatus === 'ready' ? '当前素材已完成分析，可直接用于故事板与时间线。' : visualStatusLabel(assetEvidence.visualAnalysisStatus))}</p>
              <div className="asset-evidence-grid">
                <article><b>{assetEvidence.durationMs !== null ? formatTimeMs(assetEvidence.durationMs) : '—'}</b><span>时长</span></article>
                <article><b>{assetEvidence.keyframes.length}</b><span>关键帧</span></article>
                <article><b>{assetEvidence.ocrEvidence.length}</b><span>OCR</span></article>
                <article><b>{assetEvidence.visualEvidence.length}</b><span>视觉</span></article>
              </div>
              {assetEvidence.keyframes.length > 0 && (
                <div className="asset-evidence-frames">
                  {assetEvidence.keyframes.map((frame) => (
                    <figure key={frame.imagePath}>
                      <img src={convertFileSrc(frame.imagePath)} alt="" />
                      <figcaption>{formatTimeMs(frame.timeMs)}</figcaption>
                    </figure>
                  ))}
                </div>
              )}
              {assetEvidence.ocrEvidence.length > 0 && (
                <div className="asset-evidence-section">
                  <strong>OCR 文本</strong>
                  {assetEvidence.ocrEvidence.map((evidence) => <p key={`${evidence.timeMs}-${evidence.text}`}><span>{formatTimeMs(evidence.timeMs)}</span>{evidence.text}</p>)}
                </div>
              )}
              {assetEvidence.visualEvidence.length > 0 && (
                <div className="asset-evidence-section">
                  <strong>视觉标签</strong>
                  {assetEvidence.visualEvidence.map((evidence, index) => <p key={`${evidence.timeMs}-${index}`}><span>{formatTimeMs(evidence.timeMs)}</span>{[...evidence.subjects, evidence.scene ?? '', ...evidence.actions, ...evidence.products].filter(Boolean).join(' · ') || '未返回可用视觉标签'}</p>)}
                </div>
              )}
            </section>
          ) : (
            <section className="asset-evidence-empty">
              <span className="eyebrow">INSPECTOR</span>
              <strong>点击一条素材查看分析证据</strong>
              <p>这里不再混入用户判断字段，只展示健康、分析和视觉证据。</p>
            </section>
          )}

          <footer className="asset-workbench__footer">
            <span className="state-dot working" />
            <p>素材管理只处理本地引用、健康状态、用户判断和证据，不直接改写源媒体。</p>
          </footer>
        </aside>
      </div>
    </section>
  )
}
