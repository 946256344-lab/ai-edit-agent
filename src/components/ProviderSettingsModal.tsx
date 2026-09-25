// Provider 设置弹窗只编辑 controller 草稿并触发显式动作，关闭时不自动保存。
import { useLayoutEffect, useRef } from 'react'
import type { ProviderController } from '../hooks/useProviderController'
import { useI18n } from '../lib/i18n'

type ProviderSettingsModalProps = {
  controller: ProviderController
}

export function ProviderSettingsModal({ controller }: ProviderSettingsModalProps) {
  const dialog = useRef<HTMLDialogElement>(null)
  const { t } = useI18n()
  const copy = t.provider
  useLayoutEffect(() => {
    const element = dialog.current
    if (controller.model.isOpen) element?.showModal()
    return () => element?.close()
  }, [controller.model.isOpen])
  if (!controller.model.isOpen) return null
  const { model, actions } = controller

  return (
    <dialog ref={dialog} className="settings-dialog" aria-label={copy.aria} onCancel={(event) => { event.preventDefault(); dialog.current?.close(); actions.close() }}>
      <section className="provider-modal">
        <button className="close-button" onClick={() => { dialog.current?.close(); actions.close() }} aria-label={t.common.close}>×</button>
        <span className="eyebrow">MODEL ACCESS</span>
        <h2>{copy.title}</h2>
        <p>{copy.intro}</p>

        <div className="provider-option chosen">
          <span>
            <strong>OpenAI OAuth</strong>
            <small>{copy.oauthHint}</small>
          </span>
          <b>{model.oauthStatus.state === 'connected' ? copy.connected : copy.experimental}</b>
        </div>
        <p className="oauth-status">{model.oauthStatus.state === 'pending' ? copy.oauthPending : model.oauthStatus.state === 'connected' ? t.backend.oauthConnectedStatus : model.oauthStatus.message ?? copy.notConnected}</p>
        <button
          className="primary-button modal-button"
          onClick={actions.connectOAuth}
          disabled={model.oauthStatus.state === 'pending' || model.oauthStatus.state === 'connected'}
        >
          {model.oauthStatus.state === 'pending' ? copy.waitingBrowser : model.oauthStatus.state === 'connected' ? copy.oauthConnected : copy.loginChatGpt}
        </button>
        {model.oauthStatus.state === 'connected' && (
          <button className="outline-button modal-button" onClick={actions.disconnectOAuth}>{copy.logout}</button>
        )}

        <div className="provider-divider" />
        <div className="provider-option chosen">
          <span>
            <strong>{copy.fishTitle}</strong>
            <small>{copy.fishHint}</small>
          </span>
          <b>{model.fishAudioStatus.keyStored ? (model.fishAudioStatus.voicesReadable ? copy.connected : copy.keyStoredUnreachable) : copy.notConfiguredBadge}</b>
        </div>
        <p className="oauth-status">{model.fishAudioStatus.lastErrorCode ? copy.fishStatusError(model.fishAudioStatus.lastErrorCode) : model.fishAudioStatus.keyStored ? (model.fishAudioStatus.voicesReadable ? copy.fishSaved : copy.fishUnreachable) : copy.notConfigured}</p>
        <form className="custom-api-form" onSubmit={actions.saveFishAudioKey}>
          <label><span>Fish Audio API Key</span><input type="password" value={model.form.fishAudioKey} onChange={(event) => actions.setFishAudioKey(event.target.value)} placeholder="Fish API Key" autoComplete="off" /></label>
          <button className="primary-button modal-button" type="submit" disabled={model.isSavingVoice || !model.form.fishAudioKey.trim()}>{model.isSavingVoice ? t.common.saving : copy.saveFish}</button>
        </form>
        {model.fishAudioStatus.importable && <button className="outline-button modal-button" onClick={actions.importFishAudioKey} disabled={model.isSavingVoice}>{copy.importFish}</button>}
        {model.fishAudioStatus.keyStored && <button className="outline-button modal-button" onClick={actions.clearFishAudioKey}>{copy.clearFish}</button>}

        <div className="provider-divider" />
        <div className="provider-option chosen">
          <span>
            <strong>{copy.customTitle}</strong>
            <small>{copy.customHint}</small>
          </span>
          <b>{model.customApiStatus.state === 'connected' ? model.customApiStatus.model ?? copy.connected : copy.customBadge}</b>
        </div>
        <p className="oauth-status">
          {model.customApiStatus.state === 'connected' ? t.backend.customConnectedStatus : model.customApiStatus.message ?? copy.notConfigured}
          {model.customApiStatus.state === 'connected' && copy.coarseVisual(model.customApiStatus.coarseVisualModel ?? copy.useMainModel)}
        </p>
        <form className="custom-api-form" onSubmit={actions.saveCustomApi}>
          <label>
            <span>Base URL</span>
            <input value={model.form.baseUrl} onChange={(event) => actions.setBaseUrl(event.target.value)} placeholder="https://api.example.com/v1" autoComplete="off" />
          </label>
          <label>
            <span>{copy.modelLabel}</span>
            <input value={model.form.model} onChange={(event) => actions.setModel(event.target.value)} placeholder={copy.modelPlaceholder} autoComplete="off" />
          </label>
          <label>
            <span>{copy.coarseLabel}</span>
            <input value={model.form.coarseVisualModel} onChange={(event) => actions.setCoarseVisualModel(event.target.value)} placeholder={copy.coarsePlaceholder} autoComplete="off" />
          </label>
          <label>
            <span>API Key</span>
            <input type="password" value={model.form.apiKey} onChange={(event) => actions.setApiKey(event.target.value)} placeholder="sk-..." autoComplete="off" />
          </label>
          <button
            className="primary-button modal-button"
            type="submit"
            disabled={model.isSaving || !model.form.baseUrl.trim() || !model.form.model.trim() || !model.form.apiKey.trim()}
          >
            {model.isSaving ? t.common.saving : copy.saveCustom}
          </button>
        </form>
        {model.customApiStatus.state === 'connected' && (
          <button className="outline-button modal-button" onClick={actions.disconnectCustomApi}>{copy.clearCustom}</button>
        )}

        <div className="provider-divider" />
        <div className="provider-option chosen">
          <span>
            <strong>{copy.elevenTitle}</strong>
            <small>{copy.elevenHint}</small>
          </span>
          <b>{model.elevenLabsStatus.keyStored ? (model.elevenLabsStatus.voicesReadable ? copy.connected : copy.keyStoredUnreachable) : copy.notConfiguredBadge}</b>
        </div>
        <p className="oauth-status">
          {model.elevenLabsStatus.lastErrorCode
            ? copy.voiceStatus(model.elevenLabsStatus.lastErrorCode)
            : model.elevenLabsStatus.keyStored
              ? (model.elevenLabsStatus.voicesReadable ? copy.elevenSaved : copy.elevenUnreadable)
              : model.elevenLabsStatus.importable
                ? copy.elevenImportable
                : copy.notConfigured}
        </p>
        <form className="custom-api-form" onSubmit={actions.saveElevenLabsKey}>
          <label>
            <span>ElevenLabs API Key</span>
            <input type="password" value={model.form.elevenLabsKey} onChange={(event) => actions.setElevenLabsKey(event.target.value)} placeholder="xi-..." autoComplete="off" />
          </label>
          <button className="primary-button modal-button" type="submit" disabled={model.isSavingVoice || !model.form.elevenLabsKey.trim()}>
            {model.isSavingVoice ? t.common.saving : copy.saveEleven}
          </button>
        </form>
        {model.elevenLabsStatus.importable && (
          <button className="outline-button modal-button" onClick={actions.importElevenLabsKey} disabled={model.isSavingVoice}>{copy.importEnv}</button>
        )}
        {model.elevenLabsStatus.keyStored && (
          <button className="outline-button modal-button" onClick={actions.clearElevenLabsKey}>{copy.clearEleven}</button>
        )}
        <button className="outline-button modal-button" onClick={() => { dialog.current?.close(); actions.close() }}>{t.common.close}</button>
      </section>
    </dialog>
  )
}
