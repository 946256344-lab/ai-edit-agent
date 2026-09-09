// 连接 Tauri WebView 调试端点，执行最小页面加载与运行时错误烟雾检查。
import assert from 'node:assert/strict'
import { writeFileSync } from 'node:fs'
import process from 'node:process'

const endpoint = process.env.TAURI_CDP_URL ?? 'http://127.0.0.1:9222/json'
const screenshotPath = process.env.TAURI_VERIFY_SCREENSHOT
const targets = await fetch(endpoint).then((response) => response.json())
const target = targets.find((candidate) => candidate.title === 'Assembly Video Agent')
assert.ok(target, 'Assembly Video Agent WebView target was not found.')

const socket = new WebSocket(target.webSocketDebuggerUrl)
await new Promise((resolve, reject) => {
  socket.addEventListener('open', resolve, { once: true })
  socket.addEventListener('error', reject, { once: true })
})

let nextId = 0
const pending = new Map()
const runtimeErrors = []

socket.addEventListener('message', (event) => {
  const message = JSON.parse(event.data)
  if (message.id && pending.has(message.id)) {
    const request = pending.get(message.id)
    pending.delete(message.id)
    if (message.error) request.reject(new Error(message.error.message))
    else request.resolve(message.result)
    return
  }
  if (message.method === 'Runtime.exceptionThrown') {
    runtimeErrors.push(message.params.exceptionDetails.text)
  }
  if (message.method === 'Runtime.consoleAPICalled' && message.params.type === 'error') {
    runtimeErrors.push(message.params.args.map((argument) => argument.value ?? argument.description).join(' '))
  }
  if (message.method === 'Log.entryAdded' && message.params.entry.level === 'error') {
    runtimeErrors.push(message.params.entry.text)
  }
})

function call(method, params = {}) {
  return new Promise((resolve, reject) => {
    const id = ++nextId
    pending.set(id, { resolve, reject })
    socket.send(JSON.stringify({ id, method, params }))
  })
}

async function evaluate(expression) {
  const response = await call('Runtime.evaluate', {
    expression,
    awaitPromise: true,
    returnByValue: true,
  })
  if (response.exceptionDetails) throw new Error(response.exceptionDetails.text)
  return response.result.value
}

async function waitFor(expression, timeoutMs = 15000) {
  const deadline = Date.now() + timeoutMs
  while (Date.now() < deadline) {
    try {
      if (await evaluate(expression)) return
    } catch {
      // Navigation can temporarily invalidate the execution context.
    }
    await new Promise((resolve) => setTimeout(resolve, 150))
  }
  throw new Error(`Timed out waiting for: ${expression}`)
}

async function clickButton(containerSelector, labelPrefix) {
  const clicked = await evaluate(`(() => {
    const container = document.querySelector(${JSON.stringify(containerSelector)})
    const button = [...(container?.querySelectorAll('button') ?? [])]
      .find((candidate) => candidate.innerText.trim().startsWith(${JSON.stringify(labelPrefix)}))
    if (!button) return false
    button.click()
    return true
  })()`)
  assert.equal(clicked, true, `Button ${labelPrefix} was not found in ${containerSelector}.`)
}

await call('Runtime.enable')
await call('Page.enable')
await call('Log.enable')
runtimeErrors.length = 0
await call('Page.reload', { ignoreCache: true })
await waitFor(`Boolean(document.querySelector('.connection-dot.ready'))`)
const initial = await evaluate(`({
  title: document.title,
  hasOverlay: Boolean(document.querySelector('.vite-error-overlay')),
  chat: document.querySelectorAll('.conversation-workspace--chat').length,
  preview: document.querySelectorAll('.rough-preview').length,
  overflow: document.documentElement.scrollWidth > window.innerWidth,
})`)
assert.equal(initial.title, 'Assembly Video Agent')
assert.equal(initial.hasOverlay, false)
assert.deepEqual([initial.chat, initial.preview], [1, 1])
assert.equal(initial.overflow, false)
await clickButton('.top-actions', '素材库')
await waitFor(`Boolean(document.querySelector('.asset-workbench'))`)
assert.equal(await evaluate(`document.querySelectorAll('.paired-workspace .rough-preview').length`), 1)
await clickButton('.top-actions', '返回粗剪')
await waitFor(`!document.querySelector('.asset-overlay')`)
await evaluate(`document.querySelector('[aria-label="模型设置"]').click()`)
await waitFor(`Boolean(document.querySelector('[role="dialog"]'))`)
assert.equal(await evaluate(`document.querySelector('[role="dialog"] h2')?.innerText`), '连接 Agent 模型')
await evaluate(`document.querySelector('[role="dialog"] [aria-label="关闭"]').click()`)
await waitFor(`!document.querySelector('[role="dialog"]')`)
if (screenshotPath) {
  const screenshot = await call('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false })
  writeFileSync(screenshotPath, Buffer.from(screenshot.data, 'base64'))
}
assert.deepEqual(runtimeErrors, [], `WebView runtime errors: ${runtimeErrors.join(' | ')}`)
socket.close()
console.log(JSON.stringify({ status: 'passed', initial, runtimeErrors }, null, 2))
