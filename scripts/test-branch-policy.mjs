// 用纯输入样例验证受保护分支、命名和远端基线硬门不会失效。
import assert from 'node:assert/strict'
import { evaluateBranchPolicy, evaluateBranchPolicyRatchet } from './check-branch-policy.mjs'

const config = {
  baseBranch: 'origin/master',
  protectedBranches: ['master', 'main'],
  allowedPrefixes: ['codex/', 'feature/', 'fix/'],
}

const errors = (branch, baseState = 'ancestor') => evaluateBranchPolicy({ branch, baseState }, config).join('\n')

assert.equal(errors('codex/workflow'), '')
assert.equal(errors('feature/asset-tree'), '')
assert.match(errors('master'), /禁止直接在受保护分支/)
assert.match(errors('random-name'), /不符合允许的任务分支前缀/)
assert.match(errors(''), /detached HEAD/)
assert.match(errors('fix/preview', 'missing'), /git fetch origin/)
assert.match(errors('fix/preview', 'stale'), /rebase 或 merge/)

const weakened = (mutator) => {
  const next = structuredClone({ version: 1, ...config })
  mutator(next)
  return evaluateBranchPolicyRatchet(next, { version: 1, ...config }).join('\n')
}

assert.match(weakened((next) => { next.version = 0 }), /不得降低分支策略版本/)
assert.match(weakened((next) => { next.baseBranch = 'origin/other' }), /不得更换.*远端基线/)
assert.match(weakened((next) => { next.protectedBranches = ['main'] }), /不得移除受保护分支：master/)
assert.match(weakened((next) => { next.allowedPrefixes.push('anything/') }), /不得扩大允许的分支前缀/)

console.log('分支策略检查单元测试通过。')
