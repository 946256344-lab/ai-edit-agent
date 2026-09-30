// 收集随包 runtime 的真实元数据和许可全文；外部输入只读，输出仅在当前 worktree。
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import { execFileSync } from 'node:child_process'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const flag = process.argv.indexOf('--runtime-root')
const runtime = flag < 0 ? path.join(root, 'src-tauri/resources') : path.resolve(process.argv[flag + 1])
const output = path.join(root, 'src-tauri/resources/third-party')
const licenses = path.join(output, 'licenses')
mkdirSync(licenses, { recursive: true })
const read = (file) => readFileSync(file, 'utf8').replace(/^\uFEFF/, '').replace(/\r\n/g, '\n').replace(/[\t ]+$/gm, '')
const sources = []
const texts = []
function save(name, content, source) {
  const text = `${content.replace(/^\uFEFF/, '').replace(/\r\n/g, '\n').replace(/[\t ]+$/gm, '').trimEnd()}\n`
  writeFileSync(path.join(licenses, name), text)
  sources.push({ file: `licenses/${name}`, source, sha256: createHash('sha256').update(text).digest('hex') })
  texts.push(`## ${name}\nSource: ${source}\n\n${text.trimEnd()}\n`)
}
for (const [name, relative] of [
  ['FFmpeg-GPL-3.0.txt', 'ffmpeg/LICENSE'],
  ['Python-3.12.10.txt', 'python/LICENSE.txt'],
  ['Tesseract.txt', 'tesseract/LICENSE'],
  ['DirectML-LICENSE.txt', 'directml/LICENSE.txt'],
  ['DirectML-ThirdPartyNotices.txt', 'directml/ThirdPartyNotices.txt'],
]) {
  const file = path.join(runtime, relative)
  if (!existsSync(file)) throw new Error(`Fetch runtime first: ${relative}`)
  const text = read(file)
  if (name.startsWith('FFmpeg') && !text.includes('GNU GENERAL PUBLIC LICENSE')) throw new Error('FFmpeg LICENSE is a fallback, not the GPL full text')
  save(name, text, `Fetched runtime: ${relative}; version and SHA-256 in scripts/fetch-*.ps1`)
}
save('BGE-MIT.txt', read(path.join(root, 'src-tauri/resources/models/bge-small-zh-v1.5/LICENSE')), 'Repository model LICENSE; upstream BAAI/bge-small-zh-v1.5')

for (const [name, url] of [
  ['Apache-2.0.txt', 'https://raw.githubusercontent.com/tesseract-ocr/tesseract/5.4.0/LICENSE'],
  ['CLIP-MIT.txt', 'https://raw.githubusercontent.com/openai/CLIP/main/LICENSE'],
  ['MediaInfo-24.12.txt', 'https://raw.githubusercontent.com/MediaArea/MediaInfoLib/v24.12/License.html'],
  ['ONNXRuntime-1.20.0.txt', 'https://raw.githubusercontent.com/microsoft/onnxruntime/v1.20.0/LICENSE'],
  ['ONNXRuntime-ThirdPartyNotices.txt', 'https://raw.githubusercontent.com/microsoft/onnxruntime/v1.20.0/ThirdPartyNotices.txt'],
]) {
  const cached = path.join(licenses, name)
  let content
  if (existsSync(cached)) content = read(cached)
  else {
    const response = await fetch(url)
    if (!response.ok) throw new Error(`License download ${response.status}: ${url}`)
    content = await response.text()
  }
  save(name, content, url)
}

function suppliedTexts(directory) {
  return readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name, 'en')).flatMap((entry) => {
    const file = path.join(directory, entry.name)
    if (entry.isDirectory()) return suppliedTexts(file)
    return /^(licen[sc]e|copying|copyright|notice)([._-].*)?$/i.test(entry.name) ? [file] : []
  })
}
const records = []
const site = path.join(runtime, 'python/Lib/site-packages')
for (const name of readdirSync(site).filter((name) => name.endsWith('.dist-info')).sort()) {
  const directory = path.join(site, name)
  const metadata = read(path.join(directory, 'METADATA'))
  const field = (key) => metadata.match(new RegExp(`^${key}: (.*)$`, 'm'))?.[1]?.trim()
  const pkgName = field('Name')
  const version = field('Version')
  const files = suppliedTexts(directory)
  const copyright = files.flatMap((file) => read(file).split('\n').filter((line) => /copyright\s*(\([cC]\)|©|[0-9])|©\s*[0-9]/i.test(line))).slice(0, 20).join('; ')
  const declared = field('License-Expression') || field('License') || metadata.split('\n').filter((line) => line.startsWith('Classifier: License ::')).join('; ') || (files.some((file) => read(file).includes('Apache License') && read(file).includes('Version 2.0')) ? 'Apache-2.0 (supplied license text; metadata not declared)' : '待确认 / To be confirmed')
  const record = { name: pkgName, version, pin: /^(pyJianYingDraft|pycapcut)$/i.test(pkgName) ? 'fetch script pinned' : 'actual runtime snapshot; fetch script does not pin this dependency', license: pkgName.toLowerCase() === 'pycapcut' ? '待确认 / To be confirmed (no license in package metadata)' : declared, copyright: copyright || `待确认 / To be confirmed; metadata author: ${field('Author') || field('Author-email') || 'not declared'}`, licenseFiles: files.map((file) => path.relative(site, file).replaceAll('\\', '/')) }
  records.push(record)
  texts.push(`## Python distribution: ${pkgName} ${version}\n${JSON.stringify(record, null, 2)}\n\n${files.length ? files.map((file) => `### ${path.relative(site, file)}\n\n${read(file).trimEnd()}\n`).join('\n') : 'Full license text: 待确认 / To be confirmed (not supplied).'}\n`)
}

const dlls = readdirSync(path.join(runtime, 'tesseract')).filter((name) => name.endsWith('.dll')).sort()
const ffmpegVersion = execFileSync(path.join(runtime, 'ffmpeg/ffmpeg.exe'), ['-hide_banner', '-version'], { encoding: 'utf8', windowsHide: true }).replace(/\r\n/g, '\n').replace(/[\t ]+$/gm, '')
const ffmpegLibraries = [...ffmpegVersion.matchAll(/--enable-(lib\S+|cairo|fontconfig|iconv|gnutls|lcms2|gmp|bzlib|zlib|avisynth|sdl2|frei0r|openal|chromaprint|whisper)\b/g)].map((match) => match[1])
texts.push('# FFmpeg binary build inventory\nActual fetched binary version/configuration:\n\n' + ffmpegVersion + '\nExternal library flags (component version / individual license attribution / corresponding source and full text 待确认; GPL full-build terms above do not substitute for individual notices):\n\n' + ffmpegLibraries.map((name) => `- ${name}: version 待确认; component license / copyright / full text 待确认`).join('\n'))
texts.push('# Tesseract companion DLL inventory\nVersions / copyright owners / full license terms: 待确认 / To be confirmed. The current fetch script copies all DLLs, but only the engine LICENSE. This gap requires release review.\n\n' + dlls.map((name) => `- ${name}: version 待确认; license 待确认; copyright 待确认; license full text 待确认`).join('\n'))
texts.push('# Other runtime companion binaries\nCPython DLLs and .pyd modules are covered by the Python included terms where supplied; exact binary dependency versions are not fixed in fetch-python.ps1.\n\n' + readdirSync(path.join(runtime, 'python')).filter((name) => /\.(dll|pyd)$/i.test(name) && name !== 'MediaInfo.dll').sort().map((name) => `- ${name}: version / precise attribution 待确认; see Python-3.12.10.txt; Microsoft VC runtime redistribution terms require confirmation where not supplied`).join('\n'))
writeFileSync(path.join(output, 'components.txt'), `# Collected runtime license texts and inventory\n\nGenerated by scripts/collect-runtime-notices.mjs. Public source files retained except UTF-8 BOM / CRLF / trailing whitespace normalization.\n\n${texts.join('\n')}\n`)
writeFileSync(path.join(output, 'python-distributions.json'), `${JSON.stringify(records, null, 2)}\n`)
writeFileSync(path.join(output, 'sources.json'), `${JSON.stringify(sources, null, 2)}\n`)
console.log(`Collected ${sources.length} component texts, ${records.length} Python distributions, ${dlls.length} Tesseract DLLs.`)
