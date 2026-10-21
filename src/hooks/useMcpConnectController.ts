// 本机 MCP 接入说明：解析 voycut-mcp 路径与可复制配置，不启动 MCP 进程。
import { useEffect, useState } from 'react'
import { getMcpInstallInfo } from '../lib/local-store'
import type { McpInstallInfo } from '../lib/local-store'

const PLACEHOLDER: McpInstallInfo = {
  executablePath: 'C:\\Program Files\\Voycut\\voycut-mcp.exe',
  exists: false,
  claudeCommand: 'claude mcp add voycut -- "C:\\Program Files\\Voycut\\voycut-mcp.exe"',
  codexToml: '[mcp_servers.voycut]\ncommand = "C:\\\\Program Files\\\\Voycut\\\\voycut-mcp.exe"',
}

export function useMcpConnectController(desktopRuntime: boolean) {
  const [info, setInfo] = useState<McpInstallInfo>(PLACEHOLDER)
  const [error, setError] = useState(false)
  useEffect(() => {
    if (!desktopRuntime) return
    void getMcpInstallInfo()
      .then((next) => {
        setInfo(next)
        setError(false)
      })
      .catch(() => setError(true))
  }, [desktopRuntime])
  return { model: { ...info, error } }
}

export type McpConnectController = ReturnType<typeof useMcpConnectController>
