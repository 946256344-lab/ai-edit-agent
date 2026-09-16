// 桌面窗口控制：同步最大化状态，顶栏仅展示窗口操作。
import { useEffect, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'

export function useWindowController(desktopRuntime: boolean) {
  const [maximized, setMaximized] = useState(false)

  useEffect(() => {
    if (!desktopRuntime) return
    const window = getCurrentWindow()
    const update = () => window.isMaximized().then(setMaximized)
    void update()
    const listener = window.onResized(() => { void update() })
    return () => { void listener.then((unlisten) => unlisten()) }
  }, [desktopRuntime])

  return {
    maximized,
    minimize: () => getCurrentWindow().minimize(),
    toggleMaximize: () => getCurrentWindow().toggleMaximize(),
    close: () => getCurrentWindow().close(),
  }
}
