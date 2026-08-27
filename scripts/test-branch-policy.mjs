// 用纯输入样例验证受保护分支硬门不会失效。
import assert from 'node:assert/strict'
import { evaluateBranchPolicy, evaluateBranchPolicyRatchet } from './check-branch-policy.mjs'

const config = {
  baseBranch: 'origin/master',
  protectedBranches: ['master', 'main'],
}

const errors = (branch, baseState = 'ancestor') => evaluateBranchPolicy({ branch, baseState }, config).join('\n')

assert.equal(errors('codex/workflow'), '')
assert.equal(errors('feature/asset-tree'), '')
assert.match(errors('master'), /禁止直接在受保护分支/)
assert.match(errors(''), /detached HEAD/)

const weakened = (mutator) => {
  const next = structuredClone({ version: 1, ...config })
  mutator(next)
  return evaluateBranchPolicyRatchet(next, { version: 1, ...config }).join('\n')
}

assert.match(weakened((next) => { next.version = 0 }), /不得降低分支策略版本/)
assert.match(weakened((next) => { next.protectedBranches = ['main'] }), /不得移除受保护分支：master/)

console.log('分支策略检查单元测试通过。')
