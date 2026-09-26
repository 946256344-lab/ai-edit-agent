// 调用本地 @tauri-apps/cli：补上 Cargo bin，不覆盖 npm 已注入的 PATH。
import { spawn, spawnSync } from 'node:child_process'
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
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
const scriptDir = path.dirname(fileURLToPath(import.meta.url))
const repoRoot = path.resolve(scriptDir, '..')

const extraResourceConfigs = []

function ensureFfmpegForBuild() {
  if (tauriArgs[0] !== 'build') return
  const ffmpeg = path.join(repoRoot, 'src-tauri', 'resources', 'ffmpeg', 'ffmpeg.exe')
  const ffprobe = path.join(repoRoot, 'src-tauri', 'resources', 'ffmpeg', 'ffprobe.exe')
  const license = path.join(repoRoot, 'src-tauri', 'resources', 'ffmpeg', 'LICENSE')
  if (!existsSync(ffmpeg) || !existsSync(ffprobe) || !existsSync(license)) {
    const script = path.join(repoRoot, 'scripts', 'fetch-ffmpeg.ps1')
    console.log('缺少随包 FFmpeg，正在运行 scripts/fetch-ffmpeg.ps1 …')
    const result = spawnSync('powershell.exe', ['-ExecutionPolicy', 'Bypass', '-File', script], {
      stdio: 'inherit',
      cwd: repoRoot,
      env,
    })
    if (result.status !== 0 || !existsSync(ffmpeg) || !existsSync(ffprobe)) {
      console.error('无法准备 FFmpeg。请运行：npm run ffmpeg:fetch')
      process.exit(1)
    }
  }
  extraResourceConfigs.push(path.join(repoRoot, 'src-tauri', 'tauri.ffmpeg.conf.json'))
  console.log('安装包：准备捆绑 FFmpeg/FFprobe')
}

function ensurePythonForBuild() {
  if (tauriArgs[0] !== 'build') return
  const pythonDir = path.join(repoRoot, 'src-tauri', 'resources', 'python')
  const python = path.join(pythonDir, 'python.exe')
  const mediaInfo = path.join(pythonDir, 'MediaInfo.dll')
  const sdk = path.join(pythonDir, 'Lib', 'site-packages', 'pyJianYingDraft')
  if (!existsSync(python) || !existsSync(mediaInfo) || !existsSync(sdk)) {
    const script = path.join(repoRoot, 'scripts', 'fetch-python.ps1')
    console.log('缺少随包 Python / 草稿 SDK，正在运行 scripts/fetch-python.ps1 …')
    const result = spawnSync('powershell.exe', ['-ExecutionPolicy', 'Bypass', '-File', script], {
      stdio: 'inherit',
      cwd: repoRoot,
      env,
    })
    if (result.status !== 0 || !existsSync(python) || !existsSync(sdk)) {
      console.error('无法准备 Python 运行时。请运行：npm run python:fetch')
      process.exit(1)
    }
  }
  extraResourceConfigs.push(path.join(repoRoot, 'src-tauri', 'tauri.python.conf.json'))
  console.log('安装包：准备捆绑 Python 与草稿 SDK')
}

function ensureTesseractForBuild() {
  if (tauriArgs[0] !== 'build') return
  const tesseractDir = path.join(repoRoot, 'src-tauri', 'resources', 'tesseract')
  const program = path.join(tesseractDir, 'tesseract.exe')
  const english = path.join(tesseractDir, 'tessdata', 'eng.traineddata')
  if (!existsSync(program) || !existsSync(english)) {
    const script = path.join(repoRoot, 'scripts', 'fetch-tesseract.ps1')
    console.log('缺少随包 Tesseract / eng 数据，正在运行 scripts/fetch-tesseract.ps1 …')
    const result = spawnSync('powershell.exe', ['-ExecutionPolicy', 'Bypass', '-File', script], {
      stdio: 'inherit',
      cwd: repoRoot,
      env,
    })
    if (result.status !== 0 || !existsSync(program) || !existsSync(english)) {
      console.error('无法准备 Tesseract。请运行：npm run tesseract:fetch')
      process.exit(1)
    }
  }
  extraResourceConfigs.push(path.join(repoRoot, 'src-tauri', 'tauri.tesseract.conf.json'))
  console.log('安装包：准备捆绑 Tesseract 与英文 OCR 数据')
}

// 本地选镜模型用显卡需要随包新版 DirectML.dll；开发版缺失时只提示并回退 CPU，安装包缺失则中止。
function ensureDirectmlForRun() {
  if (tauriArgs[0] !== 'build' && tauriArgs[0] !== 'dev') return
  const dll = path.join(repoRoot, 'src-tauri', 'resources', 'directml', 'DirectML.dll')
  if (!existsSync(dll)) {
    const script = path.join(repoRoot, 'scripts', 'fetch-directml.ps1')
    console.log('缺少随包 DirectML，正在运行 scripts/fetch-directml.ps1 …')
    const result = spawnSync('powershell.exe', ['-ExecutionPolicy', 'Bypass', '-File', script], {
      stdio: 'inherit',
      cwd: repoRoot,
      env,
    })
    if (result.status !== 0 || !existsSync(dll)) {
      if (tauriArgs[0] === 'build') {
        console.error('无法准备 DirectML。请运行：npm run directml:fetch')
        process.exit(1)
      }
      console.warn('DirectML 未就绪，本地选镜模型将使用 CPU。可稍后运行：npm run directml:fetch')
      return
    }
  }
  if (tauriArgs[0] === 'build') {
    extraResourceConfigs.push(path.join(repoRoot, 'src-tauri', 'tauri.directml.conf.json'))
    console.log('安装包：准备捆绑 DirectML')
  }
}

// Release 版只读编译期的网关地址，缺失时所有模型调用都会失败，因此在构建前就拦下。
function ensureGatewayForBuild() {
  if (tauriArgs[0] !== 'build' || tauriArgs.includes('--debug')) return
  const gateway = env.FELLOWCUT_GATEWAY_BASE_URL ?? ''
  if (!/^https:\/\/[^/?#@\s]+\/api\/model\/?$/.test(gateway)) {
    console.error('正式构建需要设置 FELLOWCUT_GATEWAY_BASE_URL，形如 https://<站点>/api/model。')
    console.error(gateway ? `当前值无效：${gateway}` : '当前未设置。')
    process.exit(1)
  }
  console.log(`安装包：模型网关 ${gateway}`)
}

ensureGatewayForBuild()
ensureFfmpegForBuild()
ensurePythonForBuild()
ensureTesseractForBuild()
ensureDirectmlForRun()
if (fullModelsFlag) {
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
  extraResourceConfigs.push(path.join(repoRoot, 'src-tauri', 'tauri.full-models.conf.json'))
  console.log('完整版：准备捆绑 BGE/CLIP ONNX')
}

if (extraResourceConfigs.length > 0) {
  const baseConfig = JSON.parse(
    readFileSync(path.join(repoRoot, 'src-tauri', 'tauri.conf.json'), 'utf8'),
  )
  const resources = [...(baseConfig.bundle?.resources ?? [])]
  for (const configPath of extraResourceConfigs) {
    const json = JSON.parse(readFileSync(configPath, 'utf8'))
    resources.push(...(json.bundle?.resources ?? []))
  }
  const mergedDir = path.join(repoRoot, 'src-tauri', 'target')
  mkdirSync(mergedDir, { recursive: true })
  const mergedPath = path.join(mergedDir, 'tauri.merged-resources.conf.json')
  writeFileSync(mergedPath, `${JSON.stringify({ bundle: { resources } }, null, 2)}\n`)
  tauriArgs.push('--config', mergedPath)
  console.log(`安装包：写入合并资源 ${mergedPath}（${resources.length} 项）`)
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
