// 工作台线性图标：统一导航与剪辑操作的笔画，不承载状态或行为。
const paths = {
  play: 'm8 5 11 7-11 7V5Z',
  pause: 'M8 5v14M16 5v14',
  volume: 'M3 9h4l5-4v14l-5-4H3V9Zm13-1a6 6 0 0 1 0 8m3-11a10 10 0 0 1 0 14',
  muted: 'M3 9h4l5-4v14l-5-4H3V9Zm13 0 5 6m-5 0 5-6',
  fullscreen: 'M8 3H3v5m13-5h5v5M3 16v5h5m8 0h5v-5',
  folder: 'M3 7V5a1 1 0 0 1 1-1h5l2 3h9a1 1 0 0 1 1 1v11a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V7Z',
  library: 'M4 4h16v16H4V4Zm0 12 5-5 4 4 3-3 4 4M8 8h.01',
  film: 'M4 3h16v18H4V3Zm4 0v18M16 3v18M4 8h4M4 16h4M16 8h4M16 16h4',
  plus: 'M12 5v14M5 12h14',
  chevron: 'm7 10 5 5 5-5',
  model: 'm12 3 9 5v8l-9 5-9-5V8l9-5Zm0 9 9-4M12 12 3 8m9 4v9',
  settings: 'M9 3h6l1 3 3 1 2 5-2 5-3 1-1 3H9l-1-3-3-1-2-5 2-5 3-1 1-3Zm3 5a4 4 0 1 0 0 8 4 4 0 0 0 0-8',
  arrow: 'M5 12h14m-6-6 6 6-6 6',
  undo: 'm8 5-5 5 5 5M3 10h12a5 5 0 0 1 0 10h-3',
  redo: 'm16 5 5 5-5 5M21 10H9a5 5 0 0 0 0 10h3',
  close: 'm6 6 12 12M6 18 18 6',
}

export function WorkspaceIcon({ name }: { name: keyof typeof paths }) {
  return <svg className="workspace-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>
}
