// 补齐包内缺失的许可文本：Cargo 用打包记录的提交定位上游，不把最新版当作固定版本。
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import path from 'node:path'
import os from 'node:os'
import { fileURLToPath } from 'node:url'
import { execFileSync } from 'node:child_process'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const output = path.join(root, 'src-tauri/resources/third-party')
const destination = path.join(output, 'supplements')
mkdirSync(destination, { recursive: true })
const readJson = (file) => JSON.parse(readFileSync(file, 'utf8'))
const cargo = process.env.CARGO || path.join(os.homedir(), '.cargo/bin', process.platform === 'win32' ? 'cargo.exe' : 'cargo')
const metadata = JSON.parse(execFileSync(cargo, ['metadata', '--offline', '--locked', '--format-version', '1', '--filter-platform', 'x86_64-pc-windows-msvc', '--manifest-path', path.join(root, 'src-tauri/Cargo.toml')], { encoding: 'utf8', maxBuffer: 32 * 1024 * 1024 }))
const manifest = new Map(metadata.packages.map((pkg) => [`${pkg.name}@${pkg.version}`, pkg.manifest_path]))
const records = []
for (const ecosystem of ['npm-production', 'cargo-production-windows']) {
  for (const item of readJson(path.join(output, `${ecosystem}.json`)).filter((item) => !item.licenseFiles.length || item.licenseFiles.some((file) => file.startsWith('supplements/')))) {
    const urls = []
    if (item.license === 'MPL-2.0') urls.push('https://www.mozilla.org/media/MPL/2.0/index.txt')
    else if (ecosystem.startsWith('cargo')) {
      const directory = path.dirname(manifest.get(`${item.name}@${item.version}`))
      const vcs = readJson(path.join(directory, '.cargo_vcs_info.json'))
      const repository = item.source.match(/https:\/\/github.com\/([^/]+\/[^/]+)/)?.[1]?.replace(/\.git$/, '')
        || (item.name === 'zune-inflate' ? 'etemesi254/zune-image' : null)
      if (repository) {
        for (const prefix of ['', vcs.path_in_vcs ? `${vcs.path_in_vcs}/` : '']) {
          for (const file of ['LICENSE', 'LICENSE-MIT', 'LICENSE-APACHE', 'LICENSE-ZLIB', 'LICENSE.md', 'LICENSE.txt']) urls.push(`https://raw.githubusercontent.com/${repository}/${vcs.git.sha1}/${prefix}${file}`)
        }
      }
    }
    const results = await Promise.allSettled([...new Set(urls)].map(async (url) => {
      const response = await fetch(url, { signal: AbortSignal.timeout(15000) })
      if (!response.ok) return null
      const text = (await response.text()).replace(/^\uFEFF/, '').replace(/\r\n/g, '\n').replace(/[\t ]+$/gm, '')
      return { url, text }
    }))
    const found = results.flatMap((result) => result.status === 'fulfilled' && result.value ? [result.value] : [])
    for (const [index, { url, text }] of found.entries()) {
      const file = `supplements/${item.name.replaceAll('/', '__')}@${item.version}-${index}.txt`
      writeFileSync(path.join(output, file), text)
      records.push({ ecosystem, name: item.name, version: item.version, file, source: url, sha256: createHash('sha256').update(text).digest('hex') })
    }
    console.log(`${item.name}@${item.version}: ${found.length} supplemental text(s)`)
  }
}
writeFileSync(path.join(output, 'supplemental-sources.json'), `${JSON.stringify(records, null, 2)}\n`)
