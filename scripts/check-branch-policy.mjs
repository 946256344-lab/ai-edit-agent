// 阻止在受保护分支提交，并确保任务分支建立在本地最新远端基线之上。
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
  if (!config.allowedPrefixes.some((prefix) => branch.startsWith(prefix))) {
    errors.push(`分支 ${branch} 不符合允许的任务分支前缀。`)
  }
  if (baseState === 'missing') {
    errors.push(`缺少本地 ${config.baseBranch}；请先执行 git fetch origin。`)
  } else if (baseState === 'stale') {
    errors.push(`${config.baseBranch} 不是当前分支祖先；请先 rebase 或 merge 最新基线。`)
  }
  return errors
}

export function evaluateBranchPolicyRatchet(config, baseline) {
  if (!baseline) return []
  const errors = []
  if (!Number.isInteger(config.version) || config.version < baseline.version) {
    errors.push('不得降低分支策略版本。')
  }
  if (config.baseBranch !== baseline.baseBranch) {
    errors.push('不得更换分支策略的远端基线。')
  }
  for (const branch of baseline.protectedBranches) {
    if (!config.protectedBranches.includes(branch)) errors.push(`不得移除受保护分支：${branch}`)
  }
  for (const prefix of config.allowedPrefixes) {
    if (!baseline.allowedPrefixes.includes(prefix)) errors.push(`不得扩大允许的分支前缀：${prefix}`)
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
  console.log(`分支策略检查通过：${branch} 基于 ${config.baseBranch}。`)
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) main()
