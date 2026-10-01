// 许可声明 controller：按需读取随包文本，失败只展示词典提示，可再次读取。
import { useEffect, useRef, useState } from 'react'
import { getThirdPartyNotices } from '../lib/local-store'

export function useThirdPartyNoticesController() {
  const [text, setText] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [failed, setFailed] = useState(false)
  const active = useRef(true)
  const pending = useRef(false)
  useEffect(() => {
    active.current = true
    return () => { active.current = false }
  }, [])

  async function load() {
    if (pending.current || text !== null) return
    pending.current = true
    setLoading(true)
    setFailed(false)
    try {
      const content = await getThirdPartyNotices()
      if (active.current) setText(content)
    } catch {
      if (active.current) setFailed(true)
    } finally {
      pending.current = false
      if (active.current) setLoading(false)
    }
  }

  return { model: { text, loading, failed }, actions: { load: () => void load() } }
}
