// 拦截前端代码里写死的中文界面文案：文案只能进 src/lib/i18n 词典；注释不受限。
// 确需匹配后端中文原文等非界面用途时，在该行末尾加 `i18n-allow` 注释并写明原因。
import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { extname, join, relative, resolve } from 'node:path'
import process from 'node:process'

const root = process.cwd()
const sourceRoot = 'src'
const dictionaryRoot = 'src/lib/i18n/'
const extensions = new Set(['.ts', '.tsx'])
const hanPattern = /\p{Script=Han}/u
const staged = process.argv.includes('--staged')

function normalizePath(filePath) {
  return filePath.replaceAll('\\', '/')
}

function listFiles() {
  if (staged) {
    return execFileSync('git', ['ls-files', sourceRoot], { cwd: root, encoding: 'utf8' })
      .split('\n').map(normalizePath).filter((filePath) => extensions.has(extname(filePath)))
  }
  const files = []
  const visit = (absolutePath) => {
    for (const entry of readdirSync(absolutePath)) {
      const child = join(absolutePath, entry)
      if (statSync(child).isDirectory()) visit(child)
      else if (extensions.has(extname(child))) files.push(normalizePath(relative(root, child)))
    }
  }
  const absoluteRoot = resolve(root, sourceRoot)
  if (existsSync(absoluteRoot)) visit(absoluteRoot)
  return files
}

function readSource(filePath) {
  if (!staged) return readFileSync(resolve(root, filePath), 'utf8')
  try {
    return execFileSync('git', ['show', `:${filePath}`], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] })
  } catch {
    return ''
  }
}

// 去掉注释但保留行号：块注释换成等量换行，行注释只认行首或空白后的 //，避免误伤 URL。
function stripComments(content) {
  return content
    .replace(/\/\*[\s\S]*?\*\//g, (block) => block.replace(/[^\n]/g, ''))
    .split(/\r?\n/)
    .map((line) => line.replace(/(^|\s)\/\/.*$/, '$1'))
}

const violations = []
for (const filePath of listFiles()) {
  if (filePath.startsWith(dictionaryRoot)) continue
  const content = readSource(filePath)
  const rawLines = content.split(/\r?\n/)
  stripComments(content).forEach((line, index) => {
    if (!hanPattern.test(line) || rawLines[index]?.includes('i18n-allow')) return
    violations.push(`${filePath}:${index + 1}: ${line.trim().slice(0, 120)}`)
  })
}

if (violations.length > 0) {
  console.error('发现写死的中文界面文案，请移入 src/lib/i18n 词典（非界面用途在行尾加 i18n-allow 注释）：')
  for (const violation of violations) console.error(`  ${violation}`)
  process.exit(1)
}
console.log('界面文案检查通过：前端代码没有写死的中文。')
