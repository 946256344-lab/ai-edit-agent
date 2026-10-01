// 确认删除回归：执行 App 的真实处理函数与确认桥，只替代桌面 IPC 和状态依赖。
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import vm from 'node:vm'
import ts from 'typescript'
import { confirm } from '@tauri-apps/plugin-dialog'

function compile(source, fileName) {
  return ts.transpileModule(source, {
    fileName,
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022 },
  }).outputText
}

test('an asynchronous cancellation keeps the session and never invokes deletion', async () => {
  let finishDialog
  const dialogResult = new Promise((resolve) => { finishDialog = resolve })
  const calls = []
  const desktopWindow = {
    __TAURI_INTERNALS__: {
      invoke: async (command, args) => {
        calls.push({ command, args })
        return dialogResult
      },
    },
    // 复现旧 WebView 的 Promise 返回值；旧代码将它误当成 true。
    confirm: () => dialogResult,
    alert: () => assert.fail('unexpected failure alert'),
  }
  const previousWindow = globalThis.window
  globalThis.window = desktopWindow
  try {
    const copy = {
      common: { confirmationTitle: 'Confirm action', confirm: 'Confirm', cancel: 'Cancel' },
      app: { deleteSessionConfirm: (title) => `Delete ${title}?` },
    }
    const bridge = { exports: {} }
    vm.runInNewContext(compile(readFileSync('src/lib/local-store.ts', 'utf8'), 'local-store.ts'), {
      exports: bridge.exports,
      window: desktopWindow,
      require: (name) => {
        if (name === '@tauri-apps/api/core') return { invoke: () => assert.fail('unexpected store command') }
        if (name === '@tauri-apps/plugin-dialog') return { confirm }
        if (name === './i18n') return { messages: () => copy }
        throw new Error(`Unexpected dependency: ${name}`)
      },
    })

    const source = ts.createSourceFile('App.tsx', readFileSync('src/App.tsx', 'utf8'), ts.ScriptTarget.Latest, true, ts.ScriptKind.TSX)
    let handler
    function visit(node) {
      if (ts.isFunctionDeclaration(node) && node.name?.text === 'deleteEditingSessionWorkspace') handler = node
      ts.forEachChild(node, visit)
    }
    visit(source)
    assert.ok(handler, 'the actual session deletion handler must exist')
    const sessions = [{ id: 'session-1', title: 'Keep this session' }]
    let deletionCount = 0
    let stateChanges = 0
    const remove = vm.runInNewContext(`${compile(handler.getText(source), 'handler.ts')}\ndeleteEditingSessionWorkspace`, {
      desktopRuntime: true,
      activeProjectId: 'project-1',
      editingSessions: sessions,
      uiMessages: () => copy,
      window: desktopWindow,
      confirmUserAction: bridge.exports.confirmUserAction,
      deleteStoredEditingSession: async () => { deletionCount += 1 },
      setEditingSessions: () => { stateChanges += 1 },
      activeEditingSessionRef: { current: null },
    })
    const pendingDeletion = remove('session-1')
    assert.equal(deletionCount, 0, 'deletion must wait for the dialog')
    finishDialog('Cancel')
    await pendingDeletion
    assert.equal(deletionCount, 0, 'cancellation must not reach the backend')
    assert.equal(stateChanges, 0, 'cancellation must keep the displayed sessions')
    assert.deepEqual(sessions, [{ id: 'session-1', title: 'Keep this session' }])
    assert.equal(calls.length, 1)
    assert.equal(calls[0].command, 'plugin:dialog|message')
    assert.equal(calls[0].args.buttons.OkCancelCustom[1], 'Cancel')
  } finally {
    if (previousWindow === undefined) delete globalThis.window
    else globalThis.window = previousWindow
  }
})
