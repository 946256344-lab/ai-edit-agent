// FellowCut 登录和试用状态展示，不授予本地模型调用权限。
import { useLayoutEffect, useRef } from 'react'
import type { useFellowCutAccountController } from '../hooks/useFellowCutAccountController'
import { useI18n } from '../lib/i18n'

const TRIAL_MS = 7 * 24 * 60 * 60 * 1000

export function FellowCutAccountModal({ controller }: { controller: ReturnType<typeof useFellowCutAccountController> }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const { model, actions } = controller
  const { t, locale } = useI18n()
  const copy = t.account
  useLayoutEffect(() => {
    const element = dialog.current
    if (model.isOpen) element?.showModal()
    return () => element?.close()
  }, [model.isOpen])
  if (!model.isOpen) return null

  const startedAt = model.status.trialStartedAt ? Date.parse(model.status.trialStartedAt) : NaN
  const expiry = Number.isFinite(startedAt) ? startedAt + TRIAL_MS : null
  const access = model.status.entitlement === 'trial'
    ? expiry === null ? copy.pendingTrial : expiry > Date.now() ? copy.trial : copy.expired
    : model.status.entitlement === 'paid' ? copy.paid : model.status.entitlement === 'disabled' ? copy.disabled : copy.noAccess

  const websiteLink = model.status.accountPageUrl
    ? <button type="button" className="outline-button" onClick={actions.openAccountPage}>{copy.openWebsite}</button>
    : null

  return <dialog ref={dialog} className="settings-dialog fellowcut-account-dialog" aria-labelledby="fellowcut-account-title" onCancel={(event) => { event.preventDefault(); actions.close() }}>
    <header><span className="eyebrow">VOYCUT ACCOUNT</span><h2 id="fellowcut-account-title">{copy.title}</h2></header>
    {model.status.state === 'signedOut' ? <form onSubmit={(event) => { event.preventDefault(); void actions.signIn() }}>
      <p>{copy.signInHint}</p>
      <label>{copy.email}<input autoFocus type="email" autoComplete="email" required value={model.email} onChange={(event) => actions.setEmail(event.target.value)} /></label>
      <label>{copy.password}<input type="password" autoComplete="current-password" required value={model.password} onChange={(event) => actions.setPassword(event.target.value)} /></label>
      <p>{copy.websiteHint}</p>
      <div className="fellowcut-account-actions"><button className="primary-button" type="submit" disabled={model.busy}>{model.busy ? copy.signingIn : copy.signIn}</button>{websiteLink}</div>
    </form> : <div>
      <dl><div><dt>{copy.email}</dt><dd>{model.status.email}</dd></div><div><dt>{copy.emailStatus}</dt><dd>{model.status.state === 'verified' ? copy.verified : copy.unverified}</dd></div>
        {model.status.state === 'verified' && <><div><dt>{copy.access}</dt><dd>{access}</dd></div>{expiry !== null && <div><dt>{copy.expiry}</dt><dd>{new Date(expiry).toLocaleString(locale)}</dd></div>}</>}
      </dl>
      <p>{model.status.state === 'unverified' ? copy.verifyHint : model.status.entitlement === null ? copy.startTrialHint : copy.serverCheckHint}</p>
      <div className="fellowcut-account-actions"><button type="button" className="outline-button" disabled={model.busy} onClick={() => void actions.refresh()}>{copy.refresh}</button><button type="button" className="outline-button" disabled={model.busy} onClick={() => void actions.signOut()}>{copy.signOut}</button>{(model.status.state === 'unverified' || model.status.entitlement === null) && websiteLink}</div>
    </div>}
    {model.error && <p role="alert" className="fellowcut-account-error">{model.error}</p>}
    <button type="button" className="fellowcut-account-close" disabled={model.busy} onClick={actions.close}>{t.common.close}</button>
  </dialog>
}
