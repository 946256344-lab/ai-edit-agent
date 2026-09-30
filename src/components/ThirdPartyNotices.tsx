// 设置里的第三方许可阅读区：纯文本保留上游全文，不解释 HTML 或执行外链。
import { useThirdPartyNoticesController } from '../hooks/useThirdPartyNoticesController'
import { useI18n } from '../lib/i18n'

export function ThirdPartyNotices() {
  const { model, actions } = useThirdPartyNoticesController()
  const { t } = useI18n()
  const copy = t.notices
  return (
    <section className="third-party-notices">
      <details onToggle={(event) => { if (event.currentTarget.open) actions.load() }}>
        <summary>{copy.title}</summary>
        <p>{copy.intro}</p>
        {model.loading && <p role="status">{t.common.loading}</p>}
        {model.failed && <div role="alert"><p>{copy.readFailed}</p><button className="outline-button" onClick={actions.load}>{copy.retry}</button></div>}
        {model.text !== null && <pre tabIndex={0} aria-label={copy.title}>{model.text}</pre>}
      </details>
    </section>
  )
}
