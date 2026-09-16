// 确认 master 可提交，且只拒绝 detached HEAD 与策略版本回退。
import assert from 'node:assert/strict'
import { evaluateBranchPolicy, evaluateBranchPolicyRatchet } from './check-branch-policy.mjs'

const config = {
  version: 2,
}

const errors = (branch) => evaluateBranchPolicy({ branch }).join('\n')

assert.equal(errors('master'), '')
assert.equal(errors('main'), '')
assert.equal(errors('codex/workflow'), '')
assert.equal(errors('feature/asset-tree'), '')
assert.match(errors(''), /detached HEAD/)

const weakened = (mutator) => {
  const next = structuredClone(config)
  mutator(next)
  return evaluateBranchPolicyRatchet(next, config).join('\n')
}

assert.match(weakened((next) => { next.version = 1 }), /不得降低分支策略版本/)
assert.equal(weakened((next) => { next.protectedBranches = [] }), '')

console.log('分支策略检查单元测试通过。')
