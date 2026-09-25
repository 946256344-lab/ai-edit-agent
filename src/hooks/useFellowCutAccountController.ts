// FellowCut 账号弹窗状态：密码仅在本次登录输入中保留，凭据持久化交给 Rust。
import { useEffect, useState } from 'react'
import { getFellowCutAccountStatus, signInFellowCut, signOutFellowCut } from '../lib/local-store'
import type { FellowCutAccountStatus } from '../lib/local-store'

const SIGNED_OUT: FellowCutAccountStatus = {
  state: 'signedOut', email: null, entitlement: null, trialStartedAt: null,
}

export function useFellowCutAccountController(desktopRuntime: boolean) {
  const [isOpen, setIsOpen] = useState(false)
  const [status, setStatus] = useState<FellowCutAccountStatus>(SIGNED_OUT)
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')

  useEffect(() => {
    if (!desktopRuntime) return
    let active = true
    void getFellowCutAccountStatus()
      .then((next) => { if (active) setStatus(next) })
      .catch((reason) => { if (active) setError(String(reason)) })
    return () => { active = false }
  }, [desktopRuntime])

  async function refresh() {
    setBusy(true); setError('')
    try { setStatus(await getFellowCutAccountStatus()) }
    catch (reason) { setError(String(reason)) }
    finally { setBusy(false) }
  }

  async function signIn() {
    setBusy(true); setError('')
    try {
      setStatus(await signInFellowCut(email, password))
      setPassword('')
    } catch (reason) { setError(String(reason)) }
    finally { setBusy(false) }
  }

  async function signOut() {
    setBusy(true); setError('')
    try { setStatus(await signOutFellowCut()); setPassword('') }
    catch (reason) { setError(String(reason)) }
    finally { setBusy(false) }
  }

  return {
    model: { isOpen, status, email, password, busy, error },
    actions: {
      open: () => { setError(''); setIsOpen(true) },
      close: () => { if (!busy) { setPassword(''); setIsOpen(false) } },
      setEmail, setPassword, signIn, signOut, refresh,
    },
  }
}
