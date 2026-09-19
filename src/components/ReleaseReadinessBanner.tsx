// 启动就绪横幅：发行检查 + 本地选镜模型后台下载进度；状态自持，不进入 App 组合层。
import { useEffect, useState } from 'react'
import { listen } from '@tauri-apps/api/event'
import {
  getReleaseReadiness,
  getRuntimeModelStatus,
  startRuntimeModelDownload,
} from '../lib/local-store'
import type { ReleaseReadinessReport, RuntimeModelStatus } from '../lib/local-store'

type ReleaseReadinessBannerProps = {
  enabled: boolean
}

function formatDownloadProgress(status: RuntimeModelStatus): string {
  const active = status.artifacts.find((item) =>
    item.state === 'downloading' || item.state === 'verifying' || item.id === status.currentId,
  )
  if (!active) return status.message
  if (active.bytesTotal && active.bytesTotal > 0) {
    const percent = Math.min(100, Math.round((active.bytesDownloaded / active.bytesTotal) * 100))
    return `${status.message}（${active.title} ${percent}%）`
  }
  if (active.bytesDownloaded > 0) {
    const mb = (active.bytesDownloaded / (1024 * 1024)).toFixed(1)
    return `${status.message}（${active.title} 已下 ${mb} MB）`
  }
  return status.message
}

export function ReleaseReadinessBanner({ enabled }: ReleaseReadinessBannerProps) {
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
              id: 'readiness',
              title: '发行检查',
              status: 'fail',
              message: '无法完成启动检查，请重启应用后重试。',
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
        ? { ...prev, overall: 'failed', message: '无法开始下载，请稍后重试。' }
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
            <strong>{modelFailed ? '本地模型下载失败' : '正在下载本地选镜模型'}</strong>
            <ul>
              <li>
                <span>{modelStatus ? formatDownloadProgress(modelStatus) : '准备中…'}</span>
              </li>
              {modelFailed ? (
                <li>
                  <span>应用会自动换官方源/国内镜像并续传；仍失败时可手动重试。选镜在此期间可降级使用。</span>
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
              {retrying ? '重试中…' : '重试下载'}
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
      ) : null}
    </>
  )
}
