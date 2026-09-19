// 调用本地 @tauri-apps/cli：补上 Cargo bin，不覆盖 npm 已注入的 PATH。
import { spawn } from 'node:child_process'
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'

import { existsSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

const args = process.argv.slice(2)
const require = createRequire(import.meta.url)
let tauriJs
try {
  tauriJs = require.resolve('@tauri-apps/cli/tauri.js')
} catch {
  console.error('未找到本地 @tauri-apps/cli。请先在项目根目录运行 npm install。')
  process.exit(1)
}

const cargoBin = path.join(os.homedir(), '.cargo', 'bin')
const env = { ...process.env }
const pathKey = Object.keys(env).find((key) => key.toLowerCase() === 'path') ?? 'PATH'
const currentPath = env[pathKey] ?? ''
if (!currentPath.split(path.delimiter).some((entry) => path.resolve(entry) === path.resolve(cargoBin))) {
  env[pathKey] = currentPath ? `${cargoBin}${path.delimiter}${currentPath}` : cargoBin
}
if (args[0] === 'dev') {
  env.NATIVE_PROVIDER_FULL_TRACE = '1'
}

const fullModelsFlag = args.includes('--full-models') || env.ASSEMBLY_BUNDLE_FULL_MODELS === '1'
const tauriArgs = args.filter((arg) => arg !== '--full-models')
if (fullModelsFlag) {
  const scriptDir = path.dirname(fileURLToPath(import.meta.url))
  const repoRoot = path.resolve(scriptDir, '..')
  const requiredOnnx = [
    'src-tauri/resources/models/bge-small-zh-v1.5/onnx/model.onnx',
    'src-tauri/resources/models/clip-ViT-B-32-vision/model.onnx',
    'src-tauri/resources/models/clip-ViT-B-32-text/model.onnx',
  ]
  const missing = requiredOnnx.filter((relative) => !existsSync(path.join(repoRoot, relative)))
  if (missing.length > 0) {
    console.error('完整版安装包需要本地 ONNX。请先运行：')
    console.error('  powershell -ExecutionPolicy Bypass -File scripts/fetch-clip-models.ps1')
    console.error('缺失文件：')
    for (const relative of missing) console.error(`  - ${relative}`)
    process.exit(1)
  }
  const mergeConfig = path.join(repoRoot, 'src-tauri', 'tauri.full-models.conf.json')
  tauriArgs.push('--config', mergeConfig)
  console.log('完整版：合并 tauri.full-models.conf.json（捆绑 BGE/CLIP ONNX）')
}

const child = spawn(process.execPath, [tauriJs, ...tauriArgs], {
  stdio: 'inherit',
  env,
})
child.on('exit', (code, signal) => {
  if (signal) {
    process.kill(process.pid, signal)
    return
  }
  process.exit(code ?? 1)
})
child.on('error', (error) => {
  console.error(error.message)
  process.exit(1)
})
