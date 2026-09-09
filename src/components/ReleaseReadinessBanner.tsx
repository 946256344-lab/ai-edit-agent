// 启动就绪横幅：展示发行检查结果，状态自持，不进入 App 组合层。
import { useEffect, useState } from 'react'
import { getReleaseReadiness } from '../lib/local-store'
import type { ReleaseReadinessReport } from '../lib/local-store'

type ReleaseReadinessBannerProps = {
  enabled: boolean
}

export function ReleaseReadinessBanner({ enabled }: ReleaseReadinessBannerProps) {
  const [report, setReport] = useState<ReleaseReadinessReport | null>(null)
  const [dismissed, setDismissed] = useState(false)

  useEffect(() => {
    if (!enabled) {
      setReport(null)
      setDismissed(false)
      return
    }
    let active = true
    void getReleaseReadiness()
      .then((next) => {
        if (active) setReport(next)
      })
      .catch(() => {
        if (active) {
          setReport({
            overall: 'blocked',
            checks: [{
              id: 'readiness',
              title: '发行检查',
              status: 'fail',
              message: '无法完成启动检查，请重启应用后重试。',
            }],
          })
        }
      })
    return () => {
      active = false
    }
  }, [enabled])

  if (!enabled || !report || dismissed || report.overall === 'ready') return null

  const problems = report.checks.filter((check) => check.status !== 'ok')
  const blocked = report.overall === 'blocked'

  return (
    <section
      className={`release-readiness ${blocked ? 'release-readiness--blocked' : 'release-readiness--degraded'}`}
      aria-live="polite"
    >
      <div>
        <strong>{blocked ? '启动检查未通过' : '启动检查有提醒'}</strong>
        <ul>
          {problems.map((check) => (
            <li key={check.id}>
              <b>{check.title}</b>
              <span>{check.message}</span>
            </li>
          ))}
        </ul>
      </div>
      <button type="button" className="outline-button" onClick={() => setDismissed(true)}>
        {blocked ? '暂时关闭' : '知道了'}
      </button>
    </section>
  )
}
