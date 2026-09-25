// 一体化窗口顶栏：可拖动面包屑、窗口操作与返回粗剪入口。
import type { WorkspaceView } from './workspace-types'
import { WorkspaceIcon } from './WorkspaceIcon'
export type WorkspaceHeaderModel = {
  projectName: string
  sessionTitle: string
  storeReady: boolean
  accountLabel: string
  view: WorkspaceView
}
type WindowControls = {
  maximized: boolean
  minimize: () => Promise<void>
  toggleMaximize: () => Promise<void>
  close: () => Promise<void>
}
export function WorkspaceHeader({ model, actions }: { model: WorkspaceHeaderModel; actions: { selectView: (view: WorkspaceView) => void; openAccount: () => void; windowControls: WindowControls } }) {
  const { selectView, openAccount, windowControls } = actions
  return <>
    <header className="topbar" data-tauri-drag-region>
      <div className="crumbs" data-tauri-drag-region title={`${model.projectName} / ${model.sessionTitle}`}>{model.projectName}<span data-tauri-drag-region>/</span><strong data-tauri-drag-region>{model.sessionTitle}</strong></div>
      <span data-tauri-drag-region className={`saved ${model.storeReady ? 'is-ready' : ''}`}>{model.storeReady ? '本地工作区' : '本地未连接'}</span>
      <button type="button" className="topbar-account" onClick={openAccount}>{model.accountLabel}</button>
      <div className="window-controls" role="group" aria-label="窗口操作">
        <button type="button" aria-label="最小化" title="最小化" onClick={windowControls.minimize}><WorkspaceIcon name="minimize" /></button>
        <button type="button" aria-label={windowControls.maximized ? '还原' : '最大化'} title={windowControls.maximized ? '还原' : '最大化'} onClick={windowControls.toggleMaximize}><WorkspaceIcon name={windowControls.maximized ? 'restore' : 'maximize'} /></button>
        <button type="button" className="window-close" aria-label="关闭窗口" title="关闭窗口" onClick={windowControls.close}><WorkspaceIcon name="close" /></button>
      </div>
    </header>
    <nav className="workspace-tabs" aria-label="工作区">
      <button className={model.view !== 'assets' ? 'selected' : ''} aria-pressed={model.view !== 'assets'} onClick={() => selectView('chat')}><WorkspaceIcon name="film" />粗剪工作台</button>
      {model.view === 'assets' && <span className="workspace-location">素材库</span>}
    </nav>
  </>
}
