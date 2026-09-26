// 一体化窗口顶栏：可拖动面包屑与窗口操作；返回剪辑由侧栏会话或素材库开关负责。
import type { WorkspaceView } from './workspace-types'
import { WorkspaceIcon } from './WorkspaceIcon'
import { useI18n } from '../lib/i18n'
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
export function WorkspaceHeader({ model, actions }: { model: WorkspaceHeaderModel; actions: { openAccount: () => void; windowControls: WindowControls } }) {
  const { openAccount, windowControls } = actions
  const { t } = useI18n()
  const copy = t.header
  const current = model.view === 'assets' ? t.sidebar.library : model.sessionTitle
  return (
    <header className="topbar" data-tauri-drag-region>
      <div className="crumbs" data-tauri-drag-region title={`${model.projectName} / ${current}`}>{model.projectName}<span data-tauri-drag-region>/</span><strong data-tauri-drag-region>{current}</strong></div>
      <span data-tauri-drag-region className={`saved ${model.storeReady ? 'is-ready' : ''}`}>{model.storeReady ? t.common.localWorkspace : t.common.localDisconnected}</span>
      <button type="button" className="topbar-account" onClick={openAccount}>{model.accountLabel}</button>
      <div className="window-controls" role="group" aria-label={copy.windowActions}>
        <button type="button" aria-label={copy.minimize} title={copy.minimize} onClick={windowControls.minimize}><WorkspaceIcon name="minimize" /></button>
        <button type="button" aria-label={windowControls.maximized ? copy.restore : copy.maximize} title={windowControls.maximized ? copy.restore : copy.maximize} onClick={windowControls.toggleMaximize}><WorkspaceIcon name={windowControls.maximized ? 'restore' : 'maximize'} /></button>
        <button type="button" className="window-close" aria-label={copy.closeWindow} title={copy.closeWindow} onClick={windowControls.close}><WorkspaceIcon name="close" /></button>
      </div>
    </header>
  )
}
