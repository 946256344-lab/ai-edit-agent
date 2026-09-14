// 粗剪播放器：轻量播放、定位与全屏控件，仅管理媒体展示状态。
import { useRef, useState } from 'react'
import type { RefObject } from 'react'
import { WorkspaceIcon } from './WorkspaceIcon'

function timestamp(seconds: number) {
  return `${String(Math.floor(seconds / 60)).padStart(2, '0')}:${String(Math.floor(seconds % 60)).padStart(2, '0')}`
}

export function RoughCutPlayer({ src, videoRef, onTimeChange }: {
  src: string
  videoRef: RefObject<HTMLVideoElement | null>
  onTimeChange: (timeMs: number) => void
}) {
  const stage = useRef<HTMLDivElement>(null)
  const [playing, setPlaying] = useState(false)
  const [muted, setMuted] = useState(false)
  const [duration, setDuration] = useState(0)
  const [position, setPosition] = useState(0)
  const [error, setError] = useState<string | null>(null)
  return <div className="cut-player" ref={stage}>
    <video ref={videoRef} src={src} playsInline muted={muted}
      onLoadedMetadata={(event) => { setDuration(event.currentTarget.duration); onTimeChange(0) }}
      onTimeUpdate={(event) => { setPosition(event.currentTarget.currentTime); onTimeChange(event.currentTarget.currentTime * 1000) }}
      onPlay={() => setPlaying(true)} onPause={() => setPlaying(false)} onEnded={() => setPlaying(false)}
      onError={() => setError('预览无法播放，请更新预览。')}
    />
    {error && <p className="player-error" role="status">{error}</p>}
    <div className="cut-player-controls">
      <button className="player-play" aria-label={playing ? '暂停' : '播放'} title={playing ? '暂停' : '播放'} onClick={() => {
        if (playing) videoRef.current?.pause()
        else void videoRef.current?.play().catch(() => setError('预览无法播放，请更新预览。'))
      }}><WorkspaceIcon name={playing ? 'pause' : 'play'} /></button>
      <input aria-label="播放进度" type="range" min={0} max={duration} step={0.01} value={position} disabled={!duration} onChange={(event) => {
        const next = Number(event.currentTarget.value)
        if (videoRef.current) videoRef.current.currentTime = next
        setPosition(next)
        onTimeChange(next * 1000)
      }} />
      <span className="player-time">{timestamp(position)} / {timestamp(duration)}</span>
      <button aria-label={muted ? '取消静音' : '静音'} title={muted ? '取消静音' : '静音'} onClick={() => setMuted(!muted)}><WorkspaceIcon name={muted ? 'muted' : 'volume'} /></button>
      <button aria-label="全屏" title="全屏" onClick={() => {
        const action = document.fullscreenElement ? document.exitFullscreen() : stage.current?.requestFullscreen()
        void action?.catch(() => setError('当前窗口无法进入全屏。'))
      }}><WorkspaceIcon name="fullscreen" /></button>
    </div>
  </div>
}
