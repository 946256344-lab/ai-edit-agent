import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { listen } from '@tauri-apps/api/event'
import { openUrl } from '@tauri-apps/plugin-opener'
import {
  clearCustomApi,
  clearExperimentalOpenAIOAuth,
  getCustomApiStatus,
  getExperimentalOpenAIOAuthStatus,
  saveCustomApi,
  startExperimentalOpenAIOAuth,
} from '../lib/local-store'
import type { CustomApiStatus, ExperimentalOAuthStatus } from '../lib/local-store'

const DISCONNECTED_OAUTH: ExperimentalOAuthStatus = {
  state: 'disconnected',
  message: null,
  experimental: true,
}

const DISCONNECTED_CUSTOM_API: CustomApiStatus = {
  state: 'disconnected',
  message: null,
  baseUrl: null,
  model: null,
  coarseVisualModel: null,
}

/**
 * Owns provider connection UI state. Credential values cross the Tauri bridge
 * only during explicit save/clear actions; persistence and provider selection
 * remain Rust responsibilities.
 */
export function useProviderController(desktopRuntime: boolean) {
  const [isOpen, setIsOpen] = useState(false)
  const [oauthStatus, setOAuthStatus] = useState<ExperimentalOAuthStatus>(DISCONNECTED_OAUTH)
  const [customApiStatus, setCustomApiStatus] = useState<CustomApiStatus>(DISCONNECTED_CUSTOM_API)
  const [baseUrl, setBaseUrl] = useState('')
  const [model, setModel] = useState('')
  const [coarseVisualModel, setCoarseVisualModel] = useState('')
  const [apiKey, setApiKey] = useState('')
  const [isSaving, setIsSaving] = useState(false)

  useEffect(() => {
    if (!desktopRuntime) return
    let stopListening: (() => void) | undefined
    const refreshStatus = () => void getExperimentalOpenAIOAuthStatus()
      .then(setOAuthStatus)
      .catch(() => setOAuthStatus({ state: 'failed', message: '无法读取 OAuth 状态。', experimental: true }))
    void listen<ExperimentalOAuthStatus>('experimental-openai-oauth-status', (event) => setOAuthStatus(event.payload))
      .then((unlisten) => { stopListening = unlisten })
    refreshStatus()
    const intervalId = window.setInterval(refreshStatus, 2000)
    return () => {
      window.clearInterval(intervalId)
      stopListening?.()
    }
  }, [desktopRuntime])

  useEffect(() => {
    if (!desktopRuntime) return
    void getCustomApiStatus()
      .then(setCustomApiStatus)
      .catch(() => setCustomApiStatus({
        state: 'failed',
        message: '无法读取自定义 API 状态。',
        baseUrl: null,
        model: null,
        coarseVisualModel: null,
      }))
  }, [desktopRuntime])

  async function connectOAuth() {
    try {
      const start = await startExperimentalOpenAIOAuth()
      setOAuthStatus({ state: 'pending', message: '请在浏览器中完成登录。', experimental: start.experimental })
      await openUrl(start.authorizationUrl)
    } catch {
      setOAuthStatus({ state: 'failed', message: '无法启动实验性 OAuth 登录。', experimental: true })
    }
  }

  async function disconnectOAuth() {
    try {
      setOAuthStatus(await clearExperimentalOpenAIOAuth())
    } catch {
      setOAuthStatus({ state: 'failed', message: '退出登录失败。', experimental: true })
    }
  }

  async function saveCustomConnection(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setIsSaving(true)
    try {
      const status = await saveCustomApi(
        baseUrl.trim(),
        model.trim(),
        coarseVisualModel.trim(),
        apiKey.trim(),
      ).catch(() => ({
        state: 'failed' as const,
        message: '保存自定义 API 凭据失败。',
        baseUrl: null,
        model: null,
        coarseVisualModel: null,
      }))
      setCustomApiStatus(status)
      if (status.state === 'connected') {
        setBaseUrl('')
        setModel('')
        setCoarseVisualModel('')
        setApiKey('')
      }
    } finally {
      setIsSaving(false)
    }
  }

  async function disconnectCustomApi() {
    try {
      setCustomApiStatus(await clearCustomApi())
    } catch {
      setCustomApiStatus({
        state: 'failed',
        message: '清除自定义 API 失败。',
        baseUrl: null,
        model: null,
        coarseVisualModel: null,
      })
    }
  }

  return {
    model: {
      isOpen,
      oauthStatus,
      customApiStatus,
      isSaving,
      providerLabel: customApiStatus.state === 'connected'
        ? '自定义 API 已连接'
        : oauthStatus.state === 'connected'
          ? 'GPT OAuth 已连接'
          : '模型未连接',
      form: { baseUrl, model, coarseVisualModel, apiKey },
    },
    actions: {
      open: () => setIsOpen(true),
      close: () => setIsOpen(false),
      connectOAuth: () => void connectOAuth(),
      disconnectOAuth: () => void disconnectOAuth(),
      saveCustomApi: (event: FormEvent<HTMLFormElement>) => void saveCustomConnection(event),
      disconnectCustomApi: () => void disconnectCustomApi(),
      setBaseUrl,
      setModel,
      setCoarseVisualModel,
      setApiKey,
    },
  }
}

export type ProviderController = ReturnType<typeof useProviderController>
