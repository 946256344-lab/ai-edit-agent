// 可开合素材目录树：expandedFolderIds 是唯一展开状态，aria-expanded 与条件渲染同步。
import { useEffect, useMemo, useRef, useState } from 'react'
import type { AssetTreeNode } from './asset-directory-model'

type AssetDirectoryTreeProps = {
  projectId: string | null
  roots: AssetTreeNode[]
  selectedDirectoryKey: string
  totalAssetCount: number
  unfiledAssetCount: number
  onSelectDirectory: (directoryKey: string) => void
}

type AssetFolderNodeProps = {
  node: AssetTreeNode
  selectedDirectoryKey: string
  expandedFolderIds: Set<string>
  toggleAssetFolder: (folderId: string) => void
  onSelectDirectory: (directoryKey: string) => void
}

function AssetFolderNode({
  node,
  selectedDirectoryKey,
  expandedFolderIds,
  toggleAssetFolder,
  onSelectDirectory,
}: AssetFolderNodeProps) {
  const hasChildren = node.children.length > 0
  const isExpanded = hasChildren && expandedFolderIds.has(node.key)
  const isSelected = node.key === selectedDirectoryKey

  const activateFolder = () => {
    onSelectDirectory(node.key)
    if (hasChildren) toggleAssetFolder(node.key)
  }

  return (
    <li className="asset-tree-node">
      <button
        type="button"
        className={`asset-tree-row ${isSelected ? 'is-active' : ''}`}
        aria-current={isSelected ? 'page' : undefined}
        aria-expanded={hasChildren ? isExpanded : undefined}
        onClick={activateFolder}
      >
        <span className="asset-tree-caret" aria-hidden="true">{hasChildren ? (isExpanded ? '▾' : '▸') : '•'}</span>
        <span className="asset-tree-name">{node.name}</span>
        <small>{node.directAssetCount}</small>
      </button>
      {isExpanded && (
        <ul className="asset-tree-children">
          {node.children.map((child) => (
            <AssetFolderNode
              key={child.key}
              node={child}
              selectedDirectoryKey={selectedDirectoryKey}
              expandedFolderIds={expandedFolderIds}
              toggleAssetFolder={toggleAssetFolder}
              onSelectDirectory={onSelectDirectory}
            />
          ))}
        </ul>
      )}
    </li>
  )
}

export function AssetDirectoryTree({
  projectId,
  roots,
  selectedDirectoryKey,
  totalAssetCount,
  unfiledAssetCount,
  onSelectDirectory,
}: AssetDirectoryTreeProps) {
  const [expandedFolderIds, setExpandedFolderIds] = useState<Set<string>>(new Set())
  const knownRootIdsRef = useRef<Set<string>>(new Set())
  const projectIdRef = useRef(projectId)
  const rootIds = useMemo(() => roots.map((root) => root.key), [roots])

  useEffect(() => {
    if (projectIdRef.current !== projectId) {
      projectIdRef.current = projectId
      knownRootIdsRef.current = new Set(rootIds)
      setExpandedFolderIds(new Set(rootIds))
      return
    }

    const newRootIds = rootIds.filter((rootId) => !knownRootIdsRef.current.has(rootId))
    knownRootIdsRef.current = new Set(rootIds)
    if (newRootIds.length === 0) return

    setExpandedFolderIds((current) => {
      const next = new Set(current)
      for (const rootId of newRootIds) next.add(rootId)
      return next
    })
  }, [projectId, rootIds])

  const toggleAssetFolder = (folderId: string) => {
    setExpandedFolderIds((current) => {
      const next = new Set(current)
      if (next.has(folderId)) next.delete(folderId)
      else next.add(folderId)
      return next
    })
  }

  return (
    <nav className="asset-tree-card" aria-label="素材目录">
      <div className="asset-tree-card__head">
        <strong>目录</strong>
        <small>按本地导入层级</small>
      </div>
      <div className="asset-tree-card__body">
        <button
          type="button"
          className={`asset-tree-row ${selectedDirectoryKey === 'all' ? 'is-active' : ''}`}
          aria-current={selectedDirectoryKey === 'all' ? 'page' : undefined}
          onClick={() => onSelectDirectory('all')}
        >
          <span className="asset-tree-caret" aria-hidden="true">◎</span>
          <span className="asset-tree-name">全部素材</span>
          <small>{totalAssetCount}</small>
        </button>
        {roots.length > 0 ? (
          <ul className="asset-tree-roots">
            {roots.map((root) => (
              <AssetFolderNode
                key={root.key}
                node={root}
                selectedDirectoryKey={selectedDirectoryKey}
                expandedFolderIds={expandedFolderIds}
                toggleAssetFolder={toggleAssetFolder}
                onSelectDirectory={onSelectDirectory}
              />
            ))}
          </ul>
        ) : (
          <p className="asset-empty-hint">导入文件夹后，这里会显示原始目录层级。</p>
        )}
        {unfiledAssetCount > 0 && (
          <button
            type="button"
            className={`asset-tree-row ${selectedDirectoryKey === '__unfiled__' ? 'is-active' : ''}`}
            aria-current={selectedDirectoryKey === '__unfiled__' ? 'page' : undefined}
            onClick={() => onSelectDirectory('__unfiled__')}
          >
            <span className="asset-tree-caret" aria-hidden="true">•</span>
            <span className="asset-tree-name">未归类素材</span>
            <small>{unfiledAssetCount}</small>
          </button>
        )}
      </div>
    </nav>
  )
}
