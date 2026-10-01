// 离线生成生产依赖许可：读取锁文件、已安装包与 Cargo metadata，不安装依赖、不修改缓存。
import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import os from 'node:os'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const output = path.join(root, 'src-tauri/resources/third-party')
mkdirSync(output, { recursive: true })
const read = (file) => readFileSync(file, 'utf8').replace(/^\uFEFF/, '').replace(/\r\n/g, '\n').replace(/[\t ]+$/gm, '')
const supplementsPath = path.join(output, 'supplemental-sources.json')
const supplements = existsSync(supplementsPath) ? JSON.parse(read(supplementsPath)) : []
const licenseName = /^(licen[sc]e|copying|copyright|notice|third.?party.?notices)([._-].*)?$/i

function licenseFiles(directory, depth = 0) {
  return readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name, 'en')).flatMap((entry) => {
    const file = path.join(directory, entry.name)
    if (entry.isFile() && licenseName.test(entry.name)) return [file]
    if (entry.isDirectory() && depth < 2 && /^(licen[sc]es?|legal|third_party)$/i.test(entry.name)) return licenseFiles(file, depth + 1)
    return []
  })
}

function record(name, version, license, authors, directory, source, extraFiles = []) {
  const files = [...new Set([...licenseFiles(directory), ...extraFiles])]
  const texts = files.map((file) => ({ file: path.relative(directory, file).replaceAll('\\', '/'), text: read(file) }))
  for (const item of supplements.filter((item) => item.name === name && item.version === version)) {
    texts.push({ file: `${item.file} (source: ${item.source})`, text: read(path.join(output, item.file)) })
  }
  const copyright = texts.flatMap(({ text }) => text.split('\n').filter((line) => /copyright\s*(\([cC]\)|©|[0-9])|©\s*[0-9]/i.test(line))).slice(0, 20)
  return { name, version, license: license || '待确认 / To be confirmed', copyright: copyright.length ? copyright.join('; ') : `待确认 / To be confirmed; metadata authors: ${authors || 'not declared'}`, source: source || '待确认 / To be confirmed', texts }
}

const lock = JSON.parse(read(path.join(root, 'package-lock.json')))
const declared = JSON.parse(read(path.join(root, 'package.json'))).dependencies
const imported = new Set()
function scanImports(directory) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name)
    if (entry.isDirectory()) scanImports(file)
    else if (/\.[cm]?[jt]sx?$/.test(entry.name) && !entry.name.endsWith('.d.ts')) {
      const source = ts.createSourceFile(file, read(file), ts.ScriptTarget.Latest, true)
      function visitImport(node) {
        const module = (ts.isImportDeclaration(node) && !node.importClause?.isTypeOnly) || (ts.isExportDeclaration(node) && !node.isTypeOnly)
          ? node.moduleSpecifier
          : ts.isCallExpression(node) && node.expression.kind === ts.SyntaxKind.ImportKeyword ? node.arguments[0] : null
        if (module && ts.isStringLiteral(module)) {
          const name = module.text.startsWith('@') ? module.text.split('/').slice(0, 2).join('/') : module.text.split('/')[0]
          if (declared[name]) imported.add(name)
        }
        ts.forEachChild(node, visitImport)
      }
      visitImport(source)
    }
  }
}
scanImports(path.join(root, 'src'))
const npmLocations = new Set()
function visitNpm(location) {
  if (npmLocations.has(location)) return
  const info = lock.packages[location]
  if (!info || info.dev || info.devOptional || info.link) throw new Error(`Missing production lock entry: ${location}`)
  npmLocations.add(location)
  for (const [name, optional] of [...Object.keys(info.dependencies || {}).map((name) => [name, false]), ...Object.keys(info.optionalDependencies || {}).map((name) => [name, true])]) {
    if (name.startsWith('@types/')) continue // 类型声明只供编译，不进入 WebView runtime。
    let parent = location
    let found
    while (true) {
      const candidate = `${parent ? `${parent}/` : ''}node_modules/${name}`
      if (lock.packages[candidate]) { found = candidate; break }
      if (!parent) break
      parent = path.posix.dirname(parent)
      if (parent === '.') parent = ''
    }
    if (found) visitNpm(found)
    else if (!optional) throw new Error(`Unresolved production dependency: ${name} from ${location}`)
  }
}
for (const name of imported) visitNpm(`node_modules/${name}`)
const npm = [...npmLocations].map((location) => {
  const info = lock.packages[location]
  const directory = path.join(root, location)
  const pkg = JSON.parse(read(path.join(directory, 'package.json')))
  if (pkg.version !== info.version) throw new Error(`Installed version differs from lock: ${location}`)
  const author = typeof pkg.author === 'string' ? pkg.author : pkg.author?.name
  return record(pkg.name, info.version, info.license || pkg.license, author, directory, typeof pkg.repository === 'string' ? pkg.repository : pkg.repository?.url)
})

const cargo = process.env.CARGO || path.join(os.homedir(), '.cargo/bin', process.platform === 'win32' ? 'cargo.exe' : 'cargo')
const metadata = JSON.parse(execFileSync(cargo, ['metadata', '--offline', '--locked', '--format-version', '1', '--filter-platform', 'x86_64-pc-windows-msvc', '--manifest-path', path.join(root, 'src-tauri/Cargo.toml')], { cwd: root, encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }))
const packages = new Map(metadata.packages.map((pkg) => [pkg.id, pkg]))
const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]))
const selected = new Set()
function visit(id) {
  if (selected.has(id)) return
  selected.add(id)
  for (const dep of nodes.get(id).deps) {
    // --filter-platform 已过滤平台；只走 normal 边，排除 build/dev 与 proc-macro（编译工具）。
    if (dep.dep_kinds.some(({ kind }) => kind === null) && !packages.get(dep.pkg).targets.some(({ kind }) => kind.includes('proc-macro'))) visit(dep.pkg)
  }
}
visit(metadata.resolve.root)
const cargoRecords = [...selected].filter((id) => id !== metadata.resolve.root).map((id) => {
  const pkg = packages.get(id)
  const directory = path.dirname(pkg.manifest_path)
  return record(pkg.name, pkg.version, pkg.license, pkg.authors.join(', '), directory, pkg.repository, pkg.license_file ? [path.resolve(directory, pkg.license_file)] : [])
})

function render(label, records) {
  records.sort((a, b) => `${a.name}@${a.version}`.localeCompare(`${b.name}@${b.version}`, 'en'))
  const text = [`# ${label}`, '', 'Generated by: node scripts/generate-third-party-notices.mjs', 'Copyright is extracted from supplied texts; metadata authors are not assumed to be copyright holders.', '', ...records.flatMap((item) => [
    `## ${item.name} ${item.version}`, `License: ${item.license}`, `Copyright: ${item.copyright}`, `Source: ${item.source}`, '',
    ...(item.texts.length ? item.texts.flatMap(({ file, text }) => [`### ${file}`, '', text.trimEnd(), '']) : ['License full text: 待确认 / To be confirmed (not present in installed package).', '']),
  ])].join('\n')
  writeFileSync(path.join(output, `${label}.txt`), `${text.trimEnd()}\n`)
  writeFileSync(path.join(output, `${label}.json`), `${JSON.stringify(records.map(({ texts, ...item }) => ({ ...item, licenseFiles: texts.map(({ file }) => file) })), null, 2)}\n`)
  return text
}

const summary = read(path.join(root, 'THIRD_PARTY_NOTICES.md'))
const components = read(path.join(output, 'components.txt'))
const npmText = render('npm-production', npm)
const cargoText = render('cargo-production-windows', cargoRecords)
writeFileSync(path.join(output, 'THIRD_PARTY_NOTICES.md'), summary)
writeFileSync(path.join(output, 'ALL.txt'), `${`${summary}\n\n${components}\n\n${npmText}\n\n${cargoText}`.trimEnd()}\n`)
console.log(`Generated notices: ${npm.length} npm production packages, ${cargoRecords.length} Windows runtime crates.`)
console.log(`Frontend production roots: ${[...imported].sort().join(', ')}; excluded unreferenced declarations: ${Object.keys(declared).filter((name) => !imported.has(name)).join(', ') || 'none'}`)
console.log(`Missing full texts: npm ${npm.filter((x) => !x.texts.length).map((x) => x.name).join(', ') || 'none'}; Cargo ${cargoRecords.filter((x) => !x.texts.length).map((x) => x.name).join(', ') || 'none'}`)
