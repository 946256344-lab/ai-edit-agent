// 阻止在受保护分支提交；任务分支仍应基于本地远端基线。
import { execFileSync, spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import process from 'node:process'

const root = process.cwd()

export function evaluateBranchPolicy({ branch, baseState }, config) {
  const errors = []
  if (!branch) {
    errors.push('当前处于 detached HEAD，必须切换到具名任务分支。')
    return errors
  }
  if (config.protectedBranches.includes(branch)) {
    errors.push(`禁止直接在受保护分支 ${branch} 提交。`)
  }
  return errors
}

export function evaluateBranchPolicyRatchet(config, baseline) {
  if (!baseline) return []
  const errors = []
  if (!Number.isInteger(config.version) || config.version < baseline.version) {
    errors.push('不得降低分支策略版本。')
  }
  for (const branch of baseline.protectedBranches) {
    if (!config.protectedBranches.includes(branch)) errors.push(`不得移除受保护分支：${branch}`)
  }
  return errors
}

function git(args) {
  return execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim()
}

function inspectBase(baseBranch) {
  const exists = spawnSync('git', ['rev-parse', '--verify', '--quiet', baseBranch], { cwd: root, stdio: 'ignore' })
  if (exists.status !== 0) return 'missing'
  const ancestor = spawnSync('git', ['merge-base', '--is-ancestor', baseBranch, 'HEAD'], { cwd: root, stdio: 'ignore' })
  return ancestor.status === 0 ? 'ancestor' : 'stale'
}

function main() {
  const config = JSON.parse(readFileSync(resolve(root, '.harness/branch-policy.json'), 'utf8'))
  const branch = git(['branch', '--show-current'])
  const errors = evaluateBranchPolicy({ branch, baseState: inspectBase(config.baseBranch) }, config)
  let baseline
  try {
    baseline = JSON.parse(git(['show', 'HEAD:.harness/branch-policy.json']))
  } catch {
    baseline = undefined
  }
  errors.push(...evaluateBranchPolicyRatchet(config, baseline))
  if (errors.length) {
    console.error('分支策略检查失败：')
    errors.forEach((error) => console.error(`- ${error}`))
    process.exitCode = 1
    return
  }
  console.log(`分支策略检查通过：${branch}。`)
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) main()
