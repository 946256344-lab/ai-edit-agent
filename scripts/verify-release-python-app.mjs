// 连接正式版 WebView，读取完整发行检查并验证所有随包运行组件。
import assert from 'node:assert/strict'
import process from 'node:process'

const endpoint = process.env.TAURI_CDP_URL ?? 'http://127.0.0.1:9222/json'

async function waitForTarget(timeoutMs = 30000) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    try {
      const targets = await fetch(endpoint).then((response) => response.json())
      const target = targets.find((candidate) => candidate.title === 'Assembly Video Agent')
      if (target) return target
    } catch {
      // WebView CDP is not listening yet.
    }
    await new Promise((resolve) => setTimeout(resolve, 250))
  }
  throw new Error('Assembly Video Agent WebView target was not found.')
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
    clearTimeout(request.timeout)
    if (message.error) request.reject(new Error(message.error.message))
    else request.resolve(message.result)
  }
})

function call(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++nextId
    const timeout = setTimeout(() => {
      if (pending.has(id)) {
        pending.delete(id)
        reject(new Error(`${method} timed out`))
      }
    }, 45000)
    pending.set(id, { resolve, reject, timeout })
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
const adapter = report.checks.find((item) => item.id === 'jianying_adapter')
assert.equal(adapter?.status, 'ok', `jianying_adapter check: ${adapter?.status} ${adapter?.message}`)
const tesseract = report.checks.find((item) => item.id === 'tesseract')
assert.equal(tesseract?.status, 'ok', `tesseract check: ${tesseract?.status} ${tesseract?.message}`)
console.log(JSON.stringify(report, null, 2))
socket.close()
