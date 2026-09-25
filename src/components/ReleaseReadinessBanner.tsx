// 启动就绪横幅：发行检查 + 本地选镜模型后台下载进度；状态自持，不进入 App 组合层。
import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import {
  getReleaseReadiness,
  getRuntimeModelStatus,
  startRuntimeModelDownload,
} from '../lib/local-store'
import type { ReleaseReadinessReport, RuntimeModelStatus } from '../lib/local-store'
import { translateKeyed, useI18n } from '../lib/i18n'
import type { Messages } from '../lib/i18n'

// 前端自拼的“检查不可用”项：渲染时按当前语言取文案，切换语言后不残留旧语言。
const LOCAL_FAILURE_ID = 'readiness-unavailable'

type ReleaseReadinessBannerProps = {
  enabled: boolean
}

function formatDownloadProgress(status: RuntimeModelStatus, t: Messages): string {
  const active = status.artifacts.find((item) =>
    item.state === 'downloading' || item.state === 'verifying' || item.id === status.currentId,
  )
  const message = translateKeyed(t.backend.models, status.messageKey, status.messageParams, status.message)
  if (!active) return message
  const title = t.backend.modelNames[active.id] ?? active.title
  if (active.bytesTotal && active.bytesTotal > 0) {
    const percent = Math.min(100, Math.round((active.bytesDownloaded / active.bytesTotal) * 100))
    return t.readiness.progressPercent(message, title, percent)
  }
  if (active.bytesDownloaded > 0) {
    const mb = (active.bytesDownloaded / (1024 * 1024)).toFixed(1)
    return t.readiness.progressMb(message, title, mb)
  }
  return message
}

export function ReleaseReadinessBanner({ enabled }: ReleaseReadinessBannerProps) {
  const { t } = useI18n()
  const copy = t.readiness
  const [report, setReport] = useState<ReleaseReadinessReport | null>(null)
  const [modelStatus, setModelStatus] = useState<RuntimeModelStatus | null>(null)
  const [dismissed, setDismissed] = useState(false)
  const [retrying, setRetrying] = useState(false)

  useEffect(() => {
    if (!enabled) {
      setReport(null)
      setModelStatus(null)
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
              id: LOCAL_FAILURE_ID,
              title: '',
              status: 'fail',
              message: '',
              messageKey: '',
              messageParams: {},
            }],
          })
        }
      })
    void getRuntimeModelStatus()
      .then((next) => {
        if (active) setModelStatus(next)
      })
      .catch(() => {
        /* 模型状态失败不挡就绪条 */
      })
    const progress = listen<RuntimeModelStatus>('runtime-model-progress', ({ payload }) => {
      if (active) {
        setModelStatus(payload)
        void getReleaseReadiness()
          .then((next) => {
            if (active) setReport(next)
          })
          .catch(() => undefined)
      }
    })
    return () => {
      active = false
      void progress.then((unlisten) => unlisten())
    }
  }, [enabled])

  const modelBusy = modelStatus
    && (modelStatus.overall === 'downloading' || modelStatus.overall === 'pending')
  const modelFailed = modelStatus?.overall === 'failed'
  const modelReady = !modelStatus || modelStatus.overall === 'ready'
  const readinessHidden = !report || dismissed || (report.overall === 'ready' && modelReady && !modelFailed)
  const showModelStrip = Boolean(modelBusy || modelFailed)

  if (!enabled || (readinessHidden && !showModelStrip)) return null

  const problems = report?.checks.filter((check) => check.status !== 'ok') ?? []
  const blocked = report?.overall === 'blocked'

  async function onRetryDownload() {
    setRetrying(true)
    try {
      const next = await startRuntimeModelDownload()
      setModelStatus(next)
    } catch {
      setModelStatus((prev) => prev
        ? { ...prev, overall: 'failed', message: copy.downloadStartFailed, messageKey: '', messageParams: {} }
        : prev)
    } finally {
      setRetrying(false)
    }
  }

  return (
    <>
      {showModelStrip ? (
        <section
          className="release-readiness release-readiness--degraded"
          aria-live="polite"
        >
          <div>
            <strong>{modelFailed ? copy.modelFailedTitle : copy.modelDownloading}</strong>
            <ul>
              <li>
                <span>{modelStatus ? formatDownloadProgress(modelStatus, t) : copy.preparing}</span>
              </li>
              {modelFailed ? (
                <li>
                  <span>{copy.fallbackHint}</span>
                </li>
              ) : null}
            </ul>
          </div>
          {modelFailed ? (
            <button
              type="button"
              className="outline-button"
              disabled={retrying}
              onClick={() => { void onRetryDownload() }}
            >
              {retrying ? copy.retrying : copy.retryDownload}
            </button>
          ) : null}
        </section>
      ) : null}
      {!readinessHidden && report ? (
        <section
          className={`release-readiness ${blocked ? 'release-readiness--blocked' : 'release-readiness--degraded'}`}
          aria-live="polite"
        >
          <div>
            <strong>{blocked ? copy.blockedTitle : copy.degradedTitle}</strong>
            <ul>
              {problems.map((check) => (
                <li key={check.id}>
                  <b>{check.id === LOCAL_FAILURE_ID ? copy.checkTitle : (t.backend.readinessTitles as Record<string, string>)[check.id] ?? check.title}</b>
                  <span>{check.id === LOCAL_FAILURE_ID ? copy.checkFailed : translateKeyed(t.backend.readiness, check.messageKey, check.messageParams, check.message)}</span>
                </li>
              ))}
            </ul>
          </div>
          <button type="button" className="outline-button" onClick={() => setDismissed(true)}>
            {blocked ? copy.dismissBlocked : copy.acknowledge}
          </button>
        </section>
      ) : null}
    </>
  )
}
