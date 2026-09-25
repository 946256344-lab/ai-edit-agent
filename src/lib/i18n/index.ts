// 界面语言唯一入口：当前语言、切换与本机偏好；组件用 useI18n()，非组件代码用 messages()。
// 偏好只是界面便利，存浏览器存储；读写失败时按系统语言回落，不影响项目数据。
import { useSyncExternalStore } from 'react'
import { en } from './en'
import { zhCN } from './zh-CN'

export type Locale = 'zh-CN' | 'en'
export type Messages = typeof zhCN

export const LOCALES: readonly Locale[] = ['zh-CN', 'en']

const catalogs: Record<Locale, Messages> = { 'zh-CN': zhCN, en }
const STORAGE_KEY = 'fellowcut.locale'
const listeners = new Set<() => void>()

function isLocale(value: unknown): value is Locale {
  return value === 'zh-CN' || value === 'en'
}

function systemLocale(): Locale {
  const languages = typeof navigator === 'undefined' ? [] : navigator.languages ?? [navigator.language]
  return languages.some((language) => language?.toLowerCase().startsWith('zh')) ? 'zh-CN' : 'en'
}

function initialLocale(): Locale {
  try {
    const stored = window.localStorage.getItem(STORAGE_KEY)
    if (isLocale(stored)) return stored
  } catch {
    // 存储不可用时按系统语言。
  }
  return systemLocale()
}

let current: Locale = initialLocale()

function syncDocumentLanguage() {
  if (typeof document !== 'undefined') document.documentElement.lang = current
}
syncDocumentLanguage()

export function getLocale(): Locale {
  return current
}

export function setLocale(next: Locale) {
  if (next === current) return
  current = next
  try {
    window.localStorage.setItem(STORAGE_KEY, next)
  } catch {
    // 本次会话仍生效，下次启动按系统语言。
  }
  syncDocumentLanguage()
  listeners.forEach((listener) => listener())
}

/** 非组件代码读取当前语言文案；组件内请用 useI18n() 以便切换时重渲染。 */
export function messages(): Messages {
  return catalogs[current]
}

function subscribe(listener: () => void) {
  listeners.add(listener)
  return () => { listeners.delete(listener) }
}

export function useI18n() {
  const locale = useSyncExternalStore(subscribe, getLocale, getLocale)
  return { t: catalogs[locale], locale, setLocale }
}

/** 按 Rust 给的稳定文案键翻译；键未知（旧版本或新增未翻译）时原样显示 Rust 的回落文案。 */
export function translateKeyed(
  table: Record<string, (params: Record<string, string>) => string>,
  key: string | null | undefined,
  params: Record<string, string> | null | undefined,
  fallback: string,
) {
  const format = key ? table[key] : undefined
  return format ? format(params ?? {}) : fallback
}

/** 时:分，跟随当前语言。 */
export function formatClockTime(timestampMs: number) {
  return new Date(timestampMs).toLocaleTimeString(current, { hour: '2-digit', minute: '2-digit' })
}

/** 名称排序，跟随当前语言的排序规则。 */
export function compareNames(left: string, right: string) {
  return left.localeCompare(right, current)
}
