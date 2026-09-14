// 回归：有场景分段但无片段标签时，素材级视觉结果仍须显示，OCR 初始收起。
import assert from 'node:assert/strict'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import ts from 'typescript'

const source = readFileSync('src/components/asset-workspace/AssetEvidenceInspector.tsx', 'utf8')
const compiled = ts.transpileModule(source.replace(/^import '\.\/asset-evidence\.css'\r?\n/m, ''), {
  compilerOptions: { jsx: ts.JsxEmit.ReactJSX, module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
}).outputText
mkdirSync('.artifacts', { recursive: true })
const modulePath = resolve('.artifacts/asset-evidence-regression.mjs')
writeFileSync(modulePath, compiled)
globalThis.window = { __TAURI_INTERNALS__: { convertFileSrc: value => `asset://localhost/${encodeURIComponent(value)}` } }
const { AssetEvidenceInspector } = await import(pathToFileURL(modulePath).href)
const evidence = {
  id: 'regression-asset', displayName: '原片.mp4', kind: 'video', mediaPath: 'D:/素材/原片.mp4',
  analysisStatus: 'ready', visualAnalysisStatus: 'ready', durationMs: 8000,
  visualAnalysisNote: null, keyframes: [],
  segments: [{ id: 's001', startMs: 0, endMs: 3000, frames: [], visualEvidence: null }],
  visualEvidence: [{ timeMs: null, subjects: ['储能设备'], scene: '工厂', actions: [], products: [], qualityNotes: [] }],
  ocrEvidence: [{ timeMs: 1000, text: '铭牌文字' }],
}
const html = renderToStaticMarkup(createElement(AssetEvidenceInspector, { evidence, onClose() {} }))
assert.match(html, /片段 1/)
assert.match(html, /储能设备 · 工厂/)
assert.match(html, /<video[^>]+controls/)
assert.match(html, /<details class="asset-detail__ocr">/)
assert.doesNotMatch(html, /<details[^>]*\sopen(?:[=>\s])/)
assert.match(html, /铭牌文字/)
console.log('素材详情回归通过：分段存在时保留素材级视觉标签，OCR 默认折叠。')
