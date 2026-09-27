// 项目设置里的品牌套件与默认转场表单；状态与保存在 useBrandKitController。
import type { useBrandKitController } from '../hooks/useBrandKitController'
import type { TransitionKind } from '../lib/local-store'
import { useI18n } from '../lib/i18n'

type BrandKit = ReturnType<typeof useBrandKitController>

const DEFAULT_PRIMARY = '#FFFFFF'
const DEFAULT_ACCENT = '#D9E2EC'

export function BrandKitSection({ brand, disabled }: { brand: BrandKit; disabled: boolean }) {
  const { t } = useI18n()
  const copy = t.projectSettings
  const { model, actions } = brand
  const { draft } = model
  const locked = disabled || model.busy || !model.kit
  const logoPreview = !draft.clearLogo && !draft.logoSourcePath ? model.kit?.logoPreview : null
  const transitionOptions: Array<[TransitionKind, string]> = [
    ['none', copy.transitionNone],
    ['crossfade', copy.transitionCrossfade],
    ['dip_to_black', copy.transitionDip],
  ]

  return (
    <div className="brand-kit">
      <div className="provider-option chosen brand-kit-head">
        <span>
          <strong>{copy.brandTitle}</strong>
          <small>{copy.brandHint}</small>
        </span>
      </div>
      <div className="brand-kit-grid">
        <label>
          <span>{copy.brandName}</span>
          <input value={draft.name} maxLength={40} disabled={locked} onChange={(event) => actions.updateText('name', event.target.value)} />
        </label>
        <label>
          <span>{copy.brandHandle}</span>
          <input value={draft.handle} maxLength={48} disabled={locked} onChange={(event) => actions.updateText('handle', event.target.value)} />
        </label>
        <label className="brand-kit-wide">
          <span>{copy.brandCta}</span>
          <input value={draft.cta} maxLength={40} placeholder={copy.brandCtaPlaceholder} disabled={locked} onChange={(event) => actions.updateText('cta', event.target.value)} />
        </label>
        <label>
          <span>{copy.brandPrimary}</span>
          <input type="color" value={draft.primaryColor || DEFAULT_PRIMARY} disabled={locked} onChange={(event) => actions.updateText('primaryColor', event.target.value.toUpperCase())} />
        </label>
        <label>
          <span>{copy.brandAccent}</span>
          <input type="color" value={draft.accentColor || DEFAULT_ACCENT} disabled={locked} onChange={(event) => actions.updateText('accentColor', event.target.value.toUpperCase())} />
        </label>
      </div>

      <div className="provider-option chosen">
        <span>
          <strong>{copy.brandLogo}</strong>
          <small>{copy.brandLogoHint}</small>
        </span>
        <div className="brand-kit-file">
          {logoPreview
            ? <img className="brand-kit-logo" src={logoPreview} alt="" />
            : <b>{model.hasLogo ? copy.brandLogoPicked : copy.brandLogoNone}</b>}
          <button className="outline-button" type="button" disabled={locked} onClick={actions.pickLogo}>{copy.brandChoose}</button>
          {model.hasLogo && <button className="outline-button" type="button" disabled={locked} onClick={actions.removeLogo}>{copy.brandRemove}</button>}
        </div>
      </div>

      <div className="provider-option chosen">
        <span>
          <strong>{copy.brandFont}</strong>
          <small>{copy.brandFontHint}</small>
        </span>
        <div className="brand-kit-file">
          <b>
            {draft.fontSourcePath
              ? copy.brandFontPicked
              : model.hasFont && model.kit?.fontName
                ? copy.brandFontSet(model.kit.fontName)
                : copy.brandFontNone}
          </b>
          <button className="outline-button" type="button" disabled={locked} onClick={actions.pickFont}>{copy.brandChoose}</button>
          {model.hasFont && <button className="outline-button" type="button" disabled={locked} onClick={actions.removeFont}>{copy.brandRemove}</button>}
        </div>
      </div>

      <label className="provider-option chosen">
        <span>
          <strong>{copy.transitionTitle}</strong>
          <small>{copy.transitionHint}</small>
        </span>
        <span className="brand-kit-transition">
          <select aria-label={copy.transitionTitle} value={draft.transitionKind} disabled={locked} onChange={(event) => actions.setTransitionKind(event.target.value as TransitionKind)}>
            {transitionOptions.map(([kind, label]) => <option key={kind} value={kind}>{label}</option>)}
          </select>
          {draft.transitionKind !== 'none' && (
            <select aria-label={copy.transitionTitle} value={draft.transitionMs} disabled={locked} onChange={(event) => actions.setTransitionMs(Number(event.target.value))}>
              {[200, 300, 500, 800].map((ms) => <option key={ms} value={ms}>{copy.transitionDuration(ms)}</option>)}
            </select>
          )}
        </span>
      </label>

      {model.notice && <p className={model.notice.tone === 'error' ? 'oauth-status brand-kit-error' : 'oauth-status'}>{model.notice.text}</p>}
      <button className="primary-button modal-button" type="button" disabled={locked} onClick={actions.save}>
        {model.busy ? t.common.processing : copy.brandSave}
      </button>
    </div>
  )
}
