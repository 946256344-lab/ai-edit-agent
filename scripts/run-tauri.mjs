// 调用本地 @tauri-apps/cli：补上 Cargo bin，不覆盖 npm 已注入的 PATH。
import { spawn } from 'node:child_process'
import { createRequire } from 'node:module'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'

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

const child = spawn(process.execPath, [tauriJs, ...args], {
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
