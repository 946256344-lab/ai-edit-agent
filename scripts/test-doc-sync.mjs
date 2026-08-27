// 验证文档同步规则仍能识别公开接口与必需文档缺失。
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { evaluateDocSync } from './check-doc-sync.mjs'

const policy = JSON.parse(readFileSync('.harness/doc-sync-policy.json', 'utf8'))
const completeFiles = [
  'src-tauri/src/store.rs',
  'docs/api.md',
  'docs/changes/2026-08-05-documentation-harness.md',
]
const completeRecord = new Map([
  ['docs/changes/2026-08-05-documentation-harness.md', 'docs/api.md'],
])

assert.deepEqual(evaluateDocSync(['src/App.tsx'], policy, new Map()).errors, [])
assert.deepEqual(evaluateDocSync(completeFiles, policy, completeRecord).errors, [])
assert.match(evaluateDocSync(['src-tauri/src/store.rs'], policy, new Map()).errors.join('\n'), /缺少必需的同步文档/)
assert.match(evaluateDocSync(['src-tauri/src/commands/media.rs'], policy, new Map()).errors.join('\n'), /缺少必需的同步文档/)
assert.match(evaluateDocSync(['src-tauri/src/agentloop/policy.rs'], policy, new Map()).errors.join('\n'), /缺少必需的同步文档/)

console.log('文档同步检查单元测试通过。')
