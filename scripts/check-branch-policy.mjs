// 提交时只拒绝 detached HEAD；允许直接在 master 提交。
import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import process from 'node:process'

const root = process.cwd()

export function evaluateBranchPolicy({ branch }) {
  const errors = []
  if (!branch) {
    errors.push('当前处于 detached HEAD，请先切换到 master 或任务分支。')
  }
  return errors
}

export function evaluateBranchPolicyRatchet(config, baseline) {
  if (!baseline) return []
  const errors = []
  if (!Number.isInteger(config.version) || config.version < baseline.version) {
    errors.push('不得降低分支策略版本。')
  }
  return errors
}

function git(args) {
  return execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).trim()
}

function main() {
  const config = JSON.parse(readFileSync(resolve(root, '.harness/branch-policy.json'), 'utf8'))
  const branch = git(['branch', '--show-current'])
  const errors = evaluateBranchPolicy({ branch })
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
  console.log(`分支策略检查通过：${branch || 'detached'}。`)
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) main()
