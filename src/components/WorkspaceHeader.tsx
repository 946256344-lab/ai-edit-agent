// 主工作台顶栏：显示项目与会话，素材管理按需打开。
import type { WorkspaceView } from './workspace-types'
export type WorkspaceHeaderModel = {
  projectName: string
  sessionTitle: string
  storeReady: boolean
  view: WorkspaceView
  assetCount: number
}
export function WorkspaceHeader({ model, selectView }: { model: WorkspaceHeaderModel; selectView: (view: WorkspaceView) => void }) {
  return <header className="topbar"><div className="crumbs">{model.projectName}<span>/</span><strong>{model.sessionTitle}</strong></div><div className="top-actions"><span className="saved">{model.storeReady ? '保存在本机' : '本地未连接'}</span><button className="outline-button" onClick={() => selectView(model.view === 'assets' ? 'chat' : 'assets')}>{model.view === 'assets' ? '返回粗剪' : `素材库 · ${model.assetCount}`}</button></div></header>
}
