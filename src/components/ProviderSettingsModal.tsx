// Provider 设置弹窗只编辑 controller 草稿并触发显式动作，关闭时不自动保存。
import type { ProviderController } from '../hooks/useProviderController'

type ProviderSettingsModalProps = {
  controller: ProviderController
}

export function ProviderSettingsModal({ controller }: ProviderSettingsModalProps) {
  if (!controller.model.isOpen) return null
  const { model, actions } = controller

  return (
    <div className="modal-backdrop" role="dialog" aria-modal="true" aria-label="模型提供商设置">
      <section className="provider-modal">
        <button className="close-button" onClick={actions.close} aria-label="关闭">x</button>
        <span className="eyebrow">MODEL ACCESS</span>
        <h2>连接 Agent 模型</h2>
        <p>AI 剪辑 MVP 需要此模型连接。项目文件与原始素材保持在本机；仅在理解需求或分析关键帧时发送最小必要数据。API Key 只保存在 Windows 凭据库。</p>

        <div className="provider-option chosen">
          <span>
            <strong>OpenAI OAuth</strong>
            <small>实验性 OpenCode 兼容流。令牌只存储在 Windows 凭据库，可能随 OpenAI 服务变更失效。</small>
          </span>
          <b>{model.oauthStatus.state === 'connected' ? '已连接' : '实验性'}</b>
        </div>
        <p className="oauth-status">{model.oauthStatus.message ?? '尚未连接。'}</p>
        <button
          className="primary-button modal-button"
          onClick={actions.connectOAuth}
          disabled={model.oauthStatus.state === 'pending' || model.oauthStatus.state === 'connected'}
        >
          {model.oauthStatus.state === 'pending' ? '等待浏览器授权' : model.oauthStatus.state === 'connected' ? 'OAuth 已连接' : '使用 ChatGPT 登录'}
        </button>
        {model.oauthStatus.state === 'connected' && (
          <button className="outline-button modal-button" onClick={actions.disconnectOAuth}>退出登录</button>
        )}

        <div className="provider-divider" />
        <div className="provider-option chosen">
          <span>
            <strong>自定义 API</strong>
            <small>任何 OpenAI 兼容的托管端点。主 Model 用于 storyboard 与 Agent；可选粗视觉 Model 仅用于批量画面分析。配置后自定义 API 会优先生效。</small>
          </span>
          <b>{model.customApiStatus.state === 'connected' ? model.customApiStatus.model ?? '已连接' : '自定义'}</b>
        </div>
        <p className="oauth-status">
          {model.customApiStatus.message ?? '尚未配置。'}
          {model.customApiStatus.state === 'connected' && ` 粗视觉：${model.customApiStatus.coarseVisualModel ?? '使用主 Model'}`}
        </p>
        <form className="custom-api-form" onSubmit={actions.saveCustomApi}>
          <label>
            <span>Base URL</span>
            <input value={model.form.baseUrl} onChange={(event) => actions.setBaseUrl(event.target.value)} placeholder="https://api.example.com/v1" autoComplete="off" />
          </label>
          <label>
            <span>Model（必填，storyboard 与 Agent）</span>
            <input value={model.form.model} onChange={(event) => actions.setModel(event.target.value)} placeholder="例如 main-model" autoComplete="off" />
          </label>
          <label>
            <span>粗视觉 Model（可选）</span>
            <input value={model.form.coarseVisualModel} onChange={(event) => actions.setCoarseVisualModel(event.target.value)} placeholder="留空则使用主 Model" autoComplete="off" />
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
            {model.isSaving ? '保存中' : '保存自定义 API'}
          </button>
        </form>
        {model.customApiStatus.state === 'connected' && (
          <button className="outline-button modal-button" onClick={actions.disconnectCustomApi}>清除自定义 API</button>
        )}

        <div className="provider-divider" />
        <div className="provider-option chosen">
          <span>
            <strong>配音（ElevenLabs）</strong>
            <small>API Key 只保存在 Windows 凭据库。保存后只探测音色列表，不会合成扣费。</small>
          </span>
          <b>{model.elevenLabsStatus.keyStored ? (model.elevenLabsStatus.voicesReadable ? '已连接' : '密钥已存') : '未配置'}</b>
        </div>
        <p className="oauth-status">
          {model.elevenLabsStatus.lastErrorCode
            ? `配音状态：${model.elevenLabsStatus.lastErrorCode}`
            : model.elevenLabsStatus.keyStored
              ? '已保存密钥。'
              : model.elevenLabsStatus.importable
                ? '检测到本机环境变量，可以导入。'
                : '尚未配置。'}
        </p>
        <form className="custom-api-form" onSubmit={actions.saveElevenLabsKey}>
          <label>
            <span>ElevenLabs API Key</span>
            <input type="password" value={model.form.elevenLabsKey} onChange={(event) => actions.setElevenLabsKey(event.target.value)} placeholder="xi-..." autoComplete="off" />
          </label>
          <button className="primary-button modal-button" type="submit" disabled={model.isSavingVoice || !model.form.elevenLabsKey.trim()}>
            {model.isSavingVoice ? '保存中' : '保存配音密钥'}
          </button>
        </form>
        {model.elevenLabsStatus.importable && (
          <button className="outline-button modal-button" onClick={actions.importElevenLabsKey} disabled={model.isSavingVoice}>从环境变量导入</button>
        )}
        {model.elevenLabsStatus.keyStored && (
          <button className="outline-button modal-button" onClick={actions.clearElevenLabsKey}>清除配音密钥</button>
        )}
        <button className="outline-button modal-button" onClick={actions.close}>关闭</button>
      </section>
    </div>
  )
}
