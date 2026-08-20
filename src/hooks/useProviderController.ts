// Provider controller：管理连接状态、设置弹窗与显式登录/保存/清除动作，不读取明文凭据。
import { useEffect, useState } from 'react'
import type { FormEvent } from 'react'
import { listen } from '@tauri-apps/api/event'
import { openUrl } from '@tauri-apps/plugin-opener'
import {
  clearCustomApi,
  clearElevenLabsApiKey,
  clearExperimentalOpenAIOAuth,
  getCustomApiStatus,
  getElevenLabsStatus,
  getExperimentalOpenAIOAuthStatus,
  importElevenLabsApiKeyFromEnvironment,
  saveCustomApi,
  saveElevenLabsApiKey,
  startExperimentalOpenAIOAuth,
} from '../lib/local-store'
import type { CustomApiStatus, ElevenLabsStatus, ExperimentalOAuthStatus } from '../lib/local-store'

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

const DISCONNECTED_ELEVENLABS: ElevenLabsStatus = {
  keyStored: false,
  voicesReadable: false,
  ttsAuthorized: null,
  lastErrorCode: null,
  importable: false,
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
  const [elevenLabsStatus, setElevenLabsStatus] = useState<ElevenLabsStatus>(DISCONNECTED_ELEVENLABS)
  const [elevenLabsKey, setElevenLabsKey] = useState('')
  const [isSaving, setIsSaving] = useState(false)
  const [isSavingVoice, setIsSavingVoice] = useState(false)

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

  useEffect(() => {
    if (!desktopRuntime) return
    void getElevenLabsStatus()
      .then(setElevenLabsStatus)
      .catch(() => setElevenLabsStatus({
        ...DISCONNECTED_ELEVENLABS,
        lastErrorCode: 'status_unreadable',
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

  async function saveVoiceKey(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setIsSavingVoice(true)
    try {
      const status = await saveElevenLabsApiKey(elevenLabsKey.trim()).catch(() => ({
        ...DISCONNECTED_ELEVENLABS,
        lastErrorCode: 'save_failed',
      }))
      setElevenLabsStatus(status)
      if (status.keyStored) setElevenLabsKey('')
    } finally {
      setIsSavingVoice(false)
    }
  }

  async function importVoiceKey() {
    setIsSavingVoice(true)
    try {
      setElevenLabsStatus(await importElevenLabsApiKeyFromEnvironment().catch(() => ({
        ...DISCONNECTED_ELEVENLABS,
        lastErrorCode: 'import_failed',
      })))
    } finally {
      setIsSavingVoice(false)
    }
  }

  async function clearVoiceKey() {
    setElevenLabsStatus(await clearElevenLabsApiKey().catch(() => ({
      ...DISCONNECTED_ELEVENLABS,
      lastErrorCode: 'clear_failed',
    })))
  }

  return {
    model: {
      isOpen,
      oauthStatus,
      customApiStatus,
      elevenLabsStatus,
      isSaving,
      isSavingVoice,
      providerLabel: customApiStatus.state === 'connected'
        ? '自定义 API 已连接'
        : oauthStatus.state === 'connected'
          ? 'GPT OAuth 已连接'
          : '模型未连接',
      form: { baseUrl, model, coarseVisualModel, apiKey, elevenLabsKey },
    },
    actions: {
      open: () => setIsOpen(true),
      close: () => setIsOpen(false),
      connectOAuth: () => void connectOAuth(),
      disconnectOAuth: () => void disconnectOAuth(),
      saveCustomApi: (event: FormEvent<HTMLFormElement>) => void saveCustomConnection(event),
      disconnectCustomApi: () => void disconnectCustomApi(),
      saveElevenLabsKey: (event: FormEvent<HTMLFormElement>) => void saveVoiceKey(event),
      importElevenLabsKey: () => void importVoiceKey(),
      clearElevenLabsKey: () => void clearVoiceKey(),
      setElevenLabsKey,
      setBaseUrl,
      setModel,
      setCoarseVisualModel,
      setApiKey,
    },
  }
}

export type ProviderController = ReturnType<typeof useProviderController>
