// 对话与预览并排：默认约七成对话，中间分界可拖动。
import { useCallback, useRef, useState } from 'react'
import type { KeyboardEvent, PointerEvent, ReactNode } from 'react'
import { useI18n } from '../lib/i18n'

const DEFAULT_RATIO = 0.72
const MIN_CHAT = 280
const MIN_PREVIEW = 240
const HANDLE = 8

type Props = {
  hidden: boolean
  chat: ReactNode
  preview: ReactNode
}

export function PairedWorkspace({ hidden, chat, preview }: Props) {
  const pane = useRef<HTMLDivElement>(null)
  const copy = useI18n().t.header
  const [ratio, setRatio] = useState(DEFAULT_RATIO)
  const [resizing, setResizing] = useState(false)

  const clampRatio = useCallback((next: number, width: number) => {
    const min = MIN_CHAT / width
    const max = 1 - (MIN_PREVIEW + HANDLE) / width
    if (!(width > 0) || max <= min) return DEFAULT_RATIO
    return Math.min(max, Math.max(min, next))
  }, [])

  function applyFromClientX(clientX: number) {
    const node = pane.current
    if (!node) return
    const rect = node.getBoundingClientRect()
    setRatio(clampRatio((clientX - rect.left) / rect.width, rect.width))
  }

  function onPointerDown(event: PointerEvent<HTMLButtonElement>) {
    if (event.button !== 0) return
    event.preventDefault()
    event.currentTarget.setPointerCapture(event.pointerId)
    setResizing(true)
    applyFromClientX(event.clientX)
  }

  function onPointerMove(event: PointerEvent<HTMLButtonElement>) {
    if (!event.currentTarget.hasPointerCapture(event.pointerId)) return
    applyFromClientX(event.clientX)
  }

  function stopResize(event: PointerEvent<HTMLButtonElement>) {
    if (!event.currentTarget.hasPointerCapture(event.pointerId)) return
    event.currentTarget.releasePointerCapture(event.pointerId)
    setResizing(false)
  }

  function onKeyDown(event: KeyboardEvent<HTMLButtonElement>) {
    const width = pane.current?.getBoundingClientRect().width ?? 0
    if (event.key === 'ArrowLeft') {
      event.preventDefault()
      setRatio((current) => clampRatio(current - 0.04, width))
    } else if (event.key === 'ArrowRight') {
      event.preventDefault()
      setRatio((current) => clampRatio(current + 0.04, width))
    } else if (event.key === 'Home') {
      event.preventDefault()
      setRatio(DEFAULT_RATIO)
    }
  }

  return (
    <div
      ref={pane}
      className={`paired-workspace${resizing ? ' is-resizing' : ''}`}
      style={{
        gridTemplateColumns: `minmax(${MIN_CHAT}px, ${ratio}fr) ${HANDLE}px minmax(${MIN_PREVIEW}px, ${Math.max(0.01, 1 - ratio)}fr)`,
      }}
      inert={hidden || undefined}
    >
      {chat}
      <button
        type="button"
        className="workspace-split"
        aria-label={copy.splitAria}
        aria-orientation="vertical"
        aria-valuemin={20}
        aria-valuemax={80}
        aria-valuenow={Math.round(ratio * 100)}
        aria-valuetext={copy.splitValue(Math.round(ratio * 100))}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={stopResize}
        onPointerCancel={stopResize}
        onKeyDown={onKeyDown}
      />
      {preview}
    </div>
  )
}
