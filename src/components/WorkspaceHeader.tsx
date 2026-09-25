// 一体化窗口顶栏：可拖动面包屑、窗口操作与返回粗剪入口。
import type { WorkspaceView } from './workspace-types'
import { WorkspaceIcon } from './WorkspaceIcon'
import { useI18n } from '../lib/i18n'
export type WorkspaceHeaderModel = {
  projectName: string
  sessionTitle: string
  storeReady: boolean
  view: WorkspaceView
}
type WindowControls = {
  maximized: boolean
  minimize: () => Promise<void>
  toggleMaximize: () => Promise<void>
  close: () => Promise<void>
}
export function WorkspaceHeader({ model, actions }: { model: WorkspaceHeaderModel; actions: { selectView: (view: WorkspaceView) => void; windowControls: WindowControls } }) {
  const { selectView, windowControls } = actions
  const { t } = useI18n()
  const copy = t.header
  return <>
    <header className="topbar" data-tauri-drag-region>
      <div className="crumbs" data-tauri-drag-region title={`${model.projectName} / ${model.sessionTitle}`}>{model.projectName}<span data-tauri-drag-region>/</span><strong data-tauri-drag-region>{model.sessionTitle}</strong></div>
      <span data-tauri-drag-region className={`saved ${model.storeReady ? 'is-ready' : ''}`}>{model.storeReady ? t.common.localWorkspace : t.common.localDisconnected}</span>
      <div className="window-controls" role="group" aria-label={copy.windowActions}>
        <button type="button" aria-label={copy.minimize} title={copy.minimize} onClick={windowControls.minimize}><WorkspaceIcon name="minimize" /></button>
        <button type="button" aria-label={windowControls.maximized ? copy.restore : copy.maximize} title={windowControls.maximized ? copy.restore : copy.maximize} onClick={windowControls.toggleMaximize}><WorkspaceIcon name={windowControls.maximized ? 'restore' : 'maximize'} /></button>
        <button type="button" className="window-close" aria-label={copy.closeWindow} title={copy.closeWindow} onClick={windowControls.close}><WorkspaceIcon name="close" /></button>
      </div>
    </header>
    <nav className="workspace-tabs" aria-label={copy.tabs}>
      <button className={model.view !== 'assets' ? 'selected' : ''} aria-pressed={model.view !== 'assets'} onClick={() => selectView('chat')}><WorkspaceIcon name="film" />{copy.workbench}</button>
      {model.view === 'assets' && <span className="workspace-location">{t.sidebar.library}</span>}
    </nav>
  </>
}
