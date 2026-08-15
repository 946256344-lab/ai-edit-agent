// 将后端安全目录键投影为可递归渲染的树，并计算祖先/后代关系。
import type { AssetDirectory } from '../../lib/local-store'

export type AssetTreeNode = {
  name: string
  key: string
  directAssetCount: number
  children: AssetTreeNode[]
}

export type AssetDirectoryTreeModel = {
  roots: AssetTreeNode[]
  nodes: Map<string, AssetTreeNode>
}

export function buildAssetDirectoryTree(directories: AssetDirectory[]): AssetDirectoryTreeModel {
  const nodes = new Map<string, AssetTreeNode>()

  for (const directory of directories) {
    nodes.set(directory.key, {
      name: directory.name,
      key: directory.key,
      directAssetCount: directory.directAssetCount,
      children: [],
    })
  }

  const roots: AssetTreeNode[] = []
  for (const directory of directories) {
    const node = nodes.get(directory.key)
    if (!node) continue
    const parent = directory.parentKey ? nodes.get(directory.parentKey) : null
    if (parent) parent.children.push(node)
    else roots.push(node)
  }

  const sortNodes = (items: AssetTreeNode[]) => {
    items.sort((left, right) => left.name.localeCompare(right.name, 'zh-CN'))
    for (const item of items) sortNodes(item.children)
  }
  sortNodes(roots)

  return { roots, nodes }
}
