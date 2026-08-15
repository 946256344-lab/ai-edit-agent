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
await waitFor(`document.body.innerText.includes('本地 SQLite 已就绪')`)

const initial = await evaluate(`({
  title: document.title,
  bodyLength: document.body.innerText.trim().length,
  hasOverlay: Boolean(document.querySelector('.vite-error-overlay, #webpack-dev-server-client-overlay')),
  chat: document.querySelectorAll('.conversation-workspace--chat').length,
  assets: document.querySelectorAll('.asset-workbench').length,
  artifacts: document.querySelectorAll('.conversation-workspace--artifacts').length,
  sendLabel: document.querySelector('.send-button')?.innerText.trim() ?? null,
  overflow: document.documentElement.scrollWidth > window.innerWidth,
})`)
assert.equal(initial.title, 'Assembly Video Agent')
assert.ok(initial.bodyLength > 200, 'Desktop UI rendered too little content.')
assert.equal(initial.hasOverlay, false, 'Vite error overlay is visible.')
assert.deepEqual([initial.chat, initial.assets, initial.artifacts], [1, 0, 0])
assert.equal(initial.sendLabel, '发送')
assert.equal(initial.overflow, false, 'The top-level desktop viewport has horizontal overflow.')

await clickButton('.mode-tabs', '素材')
await waitFor(`document.querySelectorAll('.asset-workbench').length === 1`)
await waitFor(`document.querySelector('.asset-workbench__header strong')?.innerText.includes('891')`)
await waitFor(`document.querySelector('.asset-tree-row[aria-expanded]')?.getAttribute('aria-expanded') === 'true'`)
await waitFor(`document.querySelectorAll('.asset-tree-row[aria-expanded]').length > 1`)
const treeBefore = await evaluate(`[...document.querySelectorAll('.asset-tree-row[aria-expanded]')].map((row) => ({
  name: row.querySelector('.asset-tree-name')?.innerText ?? '',
  expanded: row.getAttribute('aria-expanded'),
}))`)
assert.ok(treeBefore.length > 1, 'Expected a nested imported directory tree.')
assert.equal(treeBefore[0].expanded, 'true', 'Safe import root should be expanded on first entry.')
const collapsedIndex = treeBefore.findIndex((row) => row.expanded === 'false')
assert.ok(collapsedIndex >= 0, 'Expected at least one collapsed child directory.')
const expandedName = treeBefore[collapsedIndex].name
const expanded = await evaluate(`(() => {
  const rows = [...document.querySelectorAll('.asset-tree-row[aria-expanded]')]
  const row = rows.find((candidate) => candidate.querySelector('.asset-tree-name')?.innerText === ${JSON.stringify(expandedName)})
  if (!row) return false
  row.click()
  return true
})()`)
assert.equal(expanded, true)
await waitFor(`[...document.querySelectorAll('.asset-tree-row[aria-expanded]')].some((row) => (
  row.querySelector('.asset-tree-name')?.innerText === ${JSON.stringify(expandedName)}
  && row.getAttribute('aria-expanded') === 'true'
))`)

if (screenshotPath) {
  const screenshot = await call('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false })
  writeFileSync(screenshotPath, Buffer.from(screenshot.data, 'base64'))
}

await clickButton('.mode-tabs', '成果')
await waitFor(`document.querySelectorAll('.conversation-workspace--artifacts').length === 1`)
const artifactsMode = await evaluate(`({
  chat: document.querySelectorAll('.conversation-workspace--chat').length,
  assets: document.querySelectorAll('.asset-workbench').length,
  artifacts: document.querySelectorAll('.conversation-workspace--artifacts').length,
  workflowCount: document.querySelectorAll('.artifact-workflow').length,
})`)
assert.deepEqual([artifactsMode.chat, artifactsMode.assets, artifactsMode.artifacts], [0, 0, 1])
assert.equal(artifactsMode.workflowCount, 1)

await clickButton('.mode-tabs', 'Agent')
await waitFor(`document.querySelectorAll('.conversation-workspace--chat').length === 1`)
await clickButton('.sidebar-footer', '')
await waitFor(`document.querySelectorAll('[role="dialog"]').length === 1`)
const providerDialog = await evaluate(`({
  heading: document.querySelector('[role="dialog"] h2')?.innerText ?? null,
  passwordFields: document.querySelectorAll('[role="dialog"] input[type="password"]').length,
})`)
assert.equal(providerDialog.heading, '连接 Agent 模型')
assert.equal(providerDialog.passwordFields, 1)
const closed = await evaluate(`(() => {
  const button = document.querySelector('[role="dialog"] [aria-label="关闭"]')
  if (!button) return false
  button.click()
  return true
})()`)
assert.equal(closed, true)
await waitFor(`document.querySelectorAll('[role="dialog"]').length === 0`)

await new Promise((resolve) => setTimeout(resolve, 500))
assert.deepEqual(runtimeErrors, [], `WebView console/runtime errors: ${runtimeErrors.join(' | ')}`)

socket.close()
console.log(JSON.stringify({
  status: 'passed',
  initial,
  assetTree: {
    expandableRows: treeBefore.length,
    rootExpanded: treeBefore[0].expanded,
    toggledChild: expandedName,
  },
  artifactsMode,
  providerDialog,
  runtimeErrors,
}, null, 2))
