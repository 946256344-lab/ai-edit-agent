// 助手消息 Markdown 渲染：不执行原始 HTML，不加载远程图片；外部链接交给系统浏览器，不在应用窗口内跳转。
import Markdown from 'react-markdown'
import type { Components } from 'react-markdown'
import remarkGfm from 'remark-gfm'
import { openUrl } from '@tauri-apps/plugin-opener'

const EXTERNAL_LINK = /^(https?:|mailto:)/i

const components: Components = {
  a: ({ href, children }) => href && EXTERNAL_LINK.test(href)
    ? <a href={href} title={href} onClick={(event) => { event.preventDefault(); void openUrl(href).catch(() => undefined) }}>{children}</a>
    : <span>{children}</span>,
  // 模型给出的图片地址不可信，也不应让本地应用静默联网；只保留说明文字。
  img: ({ alt }) => alt ? <span className="message-markdown__image">[{alt}]</span> : null,
  table: ({ children }) => <div className="message-markdown__table"><table>{children}</table></div>,
}

export function MessageMarkdown({ content }: { content: string }) {
  return <div className="message-markdown"><Markdown remarkPlugins={[remarkGfm]} components={components}>{content}</Markdown></div>
}
