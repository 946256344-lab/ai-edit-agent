// 项目品牌套件与默认转场：读取、编辑草稿、选择 logo / 字体文件、保存。文件由 Rust 复制进应用数据目录。
import { useEffect, useState } from 'react'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { getBrandKit, setBrandKit } from '../lib/local-store'
import type { BrandKit, TransitionKind } from '../lib/local-store'
import { messages } from '../lib/i18n'

export type BrandKitDraft = {
  name: string
  handle: string
  cta: string
  primaryColor: string
  accentColor: string
  logoSourcePath: string | null
  clearLogo: boolean
  fontSourcePath: string | null
  clearFont: boolean
  transitionKind: TransitionKind
  transitionMs: number
}

export type BrandKitNotice = { tone: 'info' | 'error'; text: string }

type TextField = 'name' | 'handle' | 'cta' | 'primaryColor' | 'accentColor'

const EMPTY_DRAFT: BrandKitDraft = {
  name: '',
  handle: '',
  cta: '',
  primaryColor: '',
  accentColor: '',
  logoSourcePath: null,
  clearLogo: false,
  fontSourcePath: null,
  clearFont: false,
  transitionKind: 'none',
  transitionMs: 300,
}

function draftFromKit(kit: BrandKit): BrandKitDraft {
  return {
    ...EMPTY_DRAFT,
    name: kit.name,
    handle: kit.handle,
    cta: kit.cta,
    primaryColor: kit.primaryColor,
    accentColor: kit.accentColor,
    transitionKind: kit.defaultTransition.kind,
    transitionMs: kit.defaultTransition.durationMs,
  }
}

export function useBrandKitController(projectId: string | null, open: boolean) {
  const [kit, setKit] = useState<BrandKit | null>(null)
  const [draft, setDraft] = useState<BrandKitDraft>(EMPTY_DRAFT)
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState<BrandKitNotice | null>(null)

  useEffect(() => {
    if (!open || !projectId) {
      setKit(null)
      setDraft(EMPTY_DRAFT)
      setNotice(null)
      return
    }
    let active = true
    void getBrandKit(projectId)
      .then((next) => {
        if (!active) return
        setKit(next)
        setDraft(draftFromKit(next))
      })
      .catch(() => {
        if (active) setNotice({ tone: 'error', text: messages().projectSettings.loadFailed })
      })
    return () => {
      active = false
    }
  }, [open, projectId])

  async function pickFile(kind: 'logo' | 'font') {
    const copy = messages().projectSettings
    const selected = await openFileDialog({
      multiple: false,
      title: kind === 'logo' ? copy.brandLogo : copy.brandFont,
      filters: [kind === 'logo'
        ? { name: copy.brandLogo, extensions: ['png', 'jpg', 'jpeg', 'webp', 'svg'] }
        : { name: copy.brandFont, extensions: ['ttf', 'otf', 'woff', 'woff2'] }],
    })
    if (!selected || Array.isArray(selected)) return
    setDraft((current) => kind === 'logo'
      ? { ...current, logoSourcePath: selected, clearLogo: false }
      : { ...current, fontSourcePath: selected, clearFont: false })
  }

  async function save() {
    if (!projectId || busy) return
    const copy = messages().projectSettings
    setBusy(true)
    setNotice(null)
    try {
      const next = await setBrandKit(projectId, {
        name: draft.name,
        handle: draft.handle,
        cta: draft.cta,
        primaryColor: draft.primaryColor,
        accentColor: draft.accentColor,
        logoSourcePath: draft.logoSourcePath,
        clearLogo: draft.clearLogo,
        fontSourcePath: draft.fontSourcePath,
        clearFont: draft.clearFont,
        defaultTransition: { kind: draft.transitionKind, durationMs: draft.transitionMs },
      })
      setKit(next)
      setDraft(draftFromKit(next))
      setNotice({ tone: 'info', text: copy.brandSaved })
    } catch (error) {
      setNotice({ tone: 'error', text: copy.brandSaveFailed(String(error)) })
    } finally {
      setBusy(false)
    }
  }

  return {
    model: {
      kit,
      draft,
      busy,
      notice,
      /** 当前会保存的 logo 状态：新选的文件、保留已有、或移除。 */
      hasLogo: Boolean(draft.logoSourcePath) || (Boolean(kit?.logoFile) && !draft.clearLogo),
      hasFont: Boolean(draft.fontSourcePath) || (Boolean(kit?.fontFile) && !draft.clearFont),
    },
    actions: {
      updateText: (field: TextField, value: string) => setDraft((current) => ({ ...current, [field]: value })),
      setTransitionKind: (kind: TransitionKind) => setDraft((current) => ({ ...current, transitionKind: kind })),
      setTransitionMs: (ms: number) => setDraft((current) => ({ ...current, transitionMs: ms })),
      pickLogo: () => void pickFile('logo'),
      removeLogo: () => setDraft((current) => ({ ...current, logoSourcePath: null, clearLogo: true })),
      pickFont: () => void pickFile('font'),
      removeFont: () => setDraft((current) => ({ ...current, fontSourcePath: null, clearFont: true })),
      save: () => void save(),
    },
  }
}
