// 接入 Claude Code / Codex：展示本机 voycut-mcp 路径与可复制配置，不启动 MCP。
import { useState } from 'react'
import { useI18n } from '../lib/i18n'
import type { McpConnectController } from '../hooks/useMcpConnectController'
import './mcp-connect.css'

export function McpConnectPanel({ model }: { model: McpConnectController['model'] }) {
  const { t } = useI18n()
  const copy = t.mcp
  const [copied, setCopied] = useState<'path' | 'claude' | 'codex' | null>(null)
  const path = model.executablePath ?? copy.pathUnknown

  async function copyText(value: string, key: 'path' | 'claude' | 'codex') {
    try {
      await navigator.clipboard.writeText(value)
      setCopied(key)
    } catch {
      setCopied(null)
    }
  }

  return (
    <section className="mcp-connect" aria-label={copy.aria}>
      <header className="mcp-connect__header">
        <span className="panel-kicker">{copy.kicker}</span>
        <h1>{copy.title}</h1>
        <p>{copy.body}</p>
      </header>
      <dl className="mcp-connect__path">
        <div>
          <dt>{copy.pathLabel}</dt>
          <dd>
            <code title={path}>{path}</code>
            <button type="button" className="outline-button" onClick={() => void copyText(path, 'path')} disabled={!model.executablePath}>
              {copied === 'path' ? copy.copied : copy.copy}
            </button>
          </dd>
        </div>
      </dl>
      {model.error && <p className="mcp-connect__status" role="status">{copy.loadFailed}</p>}
      {!model.error && !model.exists && <p className="mcp-connect__status" role="status">{copy.missing}</p>}
      <section className="mcp-connect__snippet">
        <header>
          <h2>{copy.claudeTitle}</h2>
          <button type="button" className="outline-button" onClick={() => void copyText(model.claudeCommand, 'claude')}>
            {copied === 'claude' ? copy.copied : copy.copy}
          </button>
        </header>
        <pre><code>{model.claudeCommand}</code></pre>
      </section>
      <section className="mcp-connect__snippet">
        <header>
          <h2>{copy.codexTitle}</h2>
          <button type="button" className="outline-button" onClick={() => void copyText(model.codexToml, 'codex')}>
            {copied === 'codex' ? copy.copied : copy.copy}
          </button>
        </header>
        <pre><code>{model.codexToml}</code></pre>
      </section>
      <section className="mcp-connect__tools">
        <h2>{copy.toolsTitle}</h2>
        <p>{copy.toolsBody}</p>
        <ul>
          <li><code>list_projects</code> — {copy.tools.list_projects}</li>
          <li><code>list_assets</code> — {copy.tools.list_assets}</li>
          <li><code>search_assets</code> — {copy.tools.search_assets}</li>
          <li><code>get_asset_evidence</code> — {copy.tools.get_asset_evidence}</li>
          <li><code>create_and_deliver_timeline</code> — {copy.tools.create_and_deliver_timeline}</li>
        </ul>
      </section>
    </section>
  )
}
