// Connect to a packaged Tauri WebView on CDP 9222 and read get_release_readiness.
import assert from 'node:assert/strict'
import process from 'node:process'

const endpoint = process.env.TAURI_CDP_URL ?? 'http://127.0.0.1:9222/json'

async function waitForTarget(timeoutMs = 30000) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    try {
      const targets = await fetch(endpoint).then((response) => response.json())
      const target = targets.find((candidate) => candidate.title === 'Voycut')
      if (target) return target
    } catch {
      // WebView CDP is not listening yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 250))
  }
  throw new Error('Voycut WebView target was not found.')
}

const target = await waitForTarget()
const socket = new WebSocket(target.webSocketDebuggerUrl)
await new Promise((resolve, reject) => {
  socket.addEventListener('open', resolve, { once: true })
  socket.addEventListener('error', reject, { once: true })
})

let nextId = 0
const pending = new Map()
socket.addEventListener('message', (event) => {
  const message = JSON.parse(event.data)
  if (message.id && pending.has(message.id)) {
    const request = pending.get(message.id)
    pending.delete(message.id)
    if (message.error) request.reject(new Error(message.error.message))
    else request.resolve(message.result)
  }
})

function call(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++nextId
    pending.set(id, { resolve, reject })
    socket.send(JSON.stringify({ id, method, params }))
  })
}

await call('Runtime.enable')
const response = await call('Runtime.evaluate', {
  expression: `window.__TAURI_INTERNALS__.invoke('get_release_readiness')`,
  awaitPromise: true,
  returnByValue: true,
})
if (response.exceptionDetails) {
  throw new Error(response.exceptionDetails.text ?? 'invoke get_release_readiness failed')
}

const report = response.result.value
assert.ok(report, 'get_release_readiness returned empty')
const ffmpeg = report.checks.find((item) => item.id === 'ffmpeg')
const ffprobe = report.checks.find((item) => item.id === 'ffprobe')
assert.equal(ffmpeg?.status, 'ok', `ffmpeg check: ${ffmpeg?.status} ${ffmpeg?.message}`)
assert.equal(ffprobe?.status, 'ok', `ffprobe check: ${ffprobe?.status} ${ffprobe?.message}`)
console.log(JSON.stringify({ overall: report.overall, ffmpeg, ffprobe }, null, 2))
socket.close()
