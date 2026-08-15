import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { evaluateDocSync } from './check-doc-sync.mjs'

const policy = JSON.parse(readFileSync('.harness/doc-sync-policy.json', 'utf8'))
const completeFiles = [
  'src-tauri/src/store.rs',
  'AGENTS.md',
  'README.md',
  'TASKS.md',
  'docs/architecture.md',
  'docs/api.md',
  'docs/decisions.md',
  'docs/harness.md',
  'docs/changes/2026-08-05-documentation-harness.md',
]
const completeRecord = new Map([
  [
    'docs/changes/2026-08-05-documentation-harness.md',
    'AGENTS.md\nREADME.md\nTASKS.md\ndocs/architecture.md\ndocs/api.md\ndocs/decisions.md\ndocs/harness.md',
  ],
])

assert.deepEqual(evaluateDocSync(['src/App.tsx'], policy, new Map()).errors, [])
assert.deepEqual(evaluateDocSync(completeFiles, policy, completeRecord).errors, [])
assert.match(evaluateDocSync(['src-tauri/src/store.rs'], policy, new Map()).errors.join('\n'), /缺少必需的同步文档/)
assert.match(evaluateDocSync(['src-tauri/src/commands/media.rs'], policy, new Map()).errors.join('\n'), /缺少必需的同步文档/)
assert.match(evaluateDocSync(['src-tauri/src/custom_api.rs'], policy, new Map()).errors.join('\n'), /AGENTS\.md/)
assert.match(evaluateDocSync(['src-tauri/src/music_provider.rs'], policy, new Map()).errors.join('\n'), /docs\/decisions\.md/)
assert.match(evaluateDocSync(['scripts/check-agent-contracts.mjs'], policy, new Map()).errors.join('\n'), /docs\/harness\.md/)

console.log('文档同步检查单元测试通过。')
