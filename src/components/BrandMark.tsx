// 品牌标志：蓝紫渐变的竖条加播放三角，用于侧栏字标与助手头像；颜色取自 index.css 品牌变量，正式图标源文件到位后替换路径即可。
import { useId } from 'react'

export function BrandMark({ className = 'brand-mark' }: { className?: string }) {
  const id = useId()
  return (
    <svg className={className} viewBox="0 0 32 32" aria-hidden="true">
      <defs>
        <linearGradient id={`${id}-bar`} x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" style={{ stopColor: 'var(--brand-sky)' }} />
          <stop offset="1" style={{ stopColor: 'var(--brand-blue)' }} />
        </linearGradient>
        <linearGradient id={`${id}-play`} x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" style={{ stopColor: 'var(--brand-indigo)' }} />
          <stop offset="1" style={{ stopColor: 'var(--brand-violet)' }} />
        </linearGradient>
      </defs>
      <rect x="3" y="4.5" width="10" height="23" rx="5" fill={`url(#${id}-bar)`} />
      <path d="M10.5 8.1c0-2 2.2-3.2 3.9-2.1l12.4 8c1.6 1 1.6 3.3 0 4.3l-12.4 8c-1.7 1.1-3.9-.1-3.9-2.1V8.1Z" fill={`url(#${id}-play)`} fillOpacity=".94" />
    </svg>
  )
}
