// FellowCut 登录和试用状态展示，不授予本地模型调用权限。
import { useLayoutEffect, useRef } from 'react'
import type { useFellowCutAccountController } from '../hooks/useFellowCutAccountController'

const TRIAL_MS = 7 * 24 * 60 * 60 * 1000

export function FellowCutAccountModal({ controller }: { controller: ReturnType<typeof useFellowCutAccountController> }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const { model, actions } = controller
  useLayoutEffect(() => {
    const element = dialog.current
    if (model.isOpen) element?.showModal()
    return () => element?.close()
  }, [model.isOpen])
  if (!model.isOpen) return null

  const startedAt = model.status.trialStartedAt ? Date.parse(model.status.trialStartedAt) : NaN
  const expiry = Number.isFinite(startedAt) ? startedAt + TRIAL_MS : null
  const access = model.status.entitlement === 'trial'
    ? expiry === null ? '试用资格待核验' : expiry > Date.now() ? '试用中' : '试用已结束'
    : model.status.entitlement === 'paid' ? '已开通' : model.status.entitlement === 'disabled' ? '已停用' : '未开通'

  return <dialog ref={dialog} className="settings-dialog fellowcut-account-dialog" aria-labelledby="fellowcut-account-title" onCancel={(event) => { event.preventDefault(); actions.close() }}>
    <header><span className="eyebrow">FELLOWCUT ACCOUNT</span><h2 id="fellowcut-account-title">我的账号</h2></header>
    {model.status.state === 'signedOut' ? <form onSubmit={(event) => { event.preventDefault(); void actions.signIn() }}>
      <p>使用网站注册的 FellowCut 邮箱和密码登录。</p>
      <label>邮箱<input autoFocus type="email" autoComplete="email" required value={model.email} onChange={(event) => actions.setEmail(event.target.value)} /></label>
      <label>密码<input type="password" autoComplete="current-password" required value={model.password} onChange={(event) => actions.setPassword(event.target.value)} /></label>
      <p>注册、验证邮箱和找回密码请在 FellowCut 网站完成。</p>
      <button className="primary-button" type="submit" disabled={model.busy}>{model.busy ? '登录中…' : '登录'}</button>
    </form> : <div>
      <dl><div><dt>邮箱</dt><dd>{model.status.email}</dd></div><div><dt>邮箱状态</dt><dd>{model.status.state === 'verified' ? '已验证' : '待验证'}</dd></div>
        {model.status.state === 'verified' && <><div><dt>使用资格</dt><dd>{access}</dd></div>{expiry !== null && <div><dt>有效期</dt><dd>{new Date(expiry).toLocaleString('zh-CN')}</dd></div>}</>}
      </dl>
      <p>{model.status.state === 'unverified' ? '请先在邮箱中完成验证，再刷新账号状态。' : model.status.entitlement === null ? '请先在网站账号页开通试用，再刷新账号状态。' : '此处显示账号资格；模型调用将在服务端接入资格校验。'}</p>
      <div className="fellowcut-account-actions"><button type="button" className="outline-button" disabled={model.busy} onClick={() => void actions.refresh()}>刷新状态</button><button type="button" className="outline-button" disabled={model.busy} onClick={() => void actions.signOut()}>退出登录</button></div>
    </div>}
    {model.error && <p role="alert" className="fellowcut-account-error">{model.error}</p>}
    <button type="button" className="fellowcut-account-close" disabled={model.busy} onClick={actions.close}>关闭</button>
  </dialog>
}
