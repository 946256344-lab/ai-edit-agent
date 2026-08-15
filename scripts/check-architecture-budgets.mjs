import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { extname, join, relative, resolve } from 'node:path'
import process from 'node:process'

const root = process.cwd()
const budgetPath = '.harness/architecture-budgets.json'
const numericLimits = [
  'maxLines',
  'maxCharacters',
  'maxLineLength',
  'maxUseState',
  'maxUseEffect',
  'maxAsyncFunctions',
  'maxTopLevelProps',
]

function normalizePath(filePath) {
  return filePath.replaceAll('\\', '/')
}

function runGit(args) {
  return execFileSync('git', args, {
    cwd: root,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim()
}

function parseArguments(args) {
  if (args.length === 0) return { staged: false }
  if (args.length === 1 && args[0] === '--staged') return { staged: true }
  throw new Error(`Unknown arguments: ${args.join(' ')}`)
}

function readTarget(filePath, staged) {
  if (staged) {
    try {
      return execFileSync('git', ['show', `:${filePath}`], {
        cwd: root,
        encoding: 'utf8',
        stdio: ['ignore', 'pipe', 'pipe'],
      })
    } catch {
      return undefined
    }
  }
  const absolutePath = resolve(root, filePath)
  return existsSync(absolutePath) ? readFileSync(absolutePath, 'utf8') : undefined
}

function listDirectoryFiles(directory, extensions, staged) {
  if (staged) {
    const files = runGit(['ls-files'])
    return files
      ? files.split('\n').map(normalizePath).filter((filePath) => (
          filePath.startsWith(`${directory}/`) && extensions.includes(extname(filePath))
        ))
      : []
  }

  const absoluteDirectory = resolve(root, directory)
  if (!existsSync(absoluteDirectory)) return []
  const files = []
  const visit = (absolutePath) => {
    for (const entry of readdirSync(absolutePath)) {
      const child = join(absolutePath, entry)
      if (statSync(child).isDirectory()) visit(child)
      else if (extensions.includes(extname(child))) files.push(normalizePath(relative(root, child)))
    }
  }
  visit(absoluteDirectory)
  return files
}

function countLines(content) {
  if (!content) return 0
  return content.replace(/\r\n/g, '\n').split('\n').length - (content.endsWith('\n') ? 1 : 0)
}

function countHookCalls(content, hookName) {
  return content
    .split(/\r?\n/)
    .filter((line) => !line.trimStart().startsWith('import '))
    .reduce((count, line) => count + (line.match(new RegExp(`\\b${hookName}(?:<[^>]+>)?\\s*\\(`, 'g'))?.length ?? 0), 0)
}

function countTopLevelProps(content) {
  const match = content.match(/export function\s+\w+\s*\(\s*\{([^}]*)\}\s*:/s)
  if (!match) return null
  if (match[1].includes('...')) return null
  return match[1].split(',').map((value) => value.trim()).filter(Boolean).length
}

function maxLineLength(content) {
  return Math.max(0, ...content.replace(/\r\n/g, '\n').split('\n').map((line) => line.length))
}

function countCharacters(content) {
  return content.replace(/\r\n/g, '\n').length
}

function metrics(content) {
  return {
    lines: countLines(content),
    characters: countCharacters(content),
    maxLineLength: maxLineLength(content),
    useState: countHookCalls(content, 'useState'),
    useEffect: countHookCalls(content, 'useEffect'),
    asyncFunctions: content.match(/\basync\b/g)?.length ?? 0,
    topLevelProps: countTopLevelProps(content),
  }
}

export function evaluateArchitecture(config, contents, directoryFiles = new Map()) {
  const errors = []
  const checkedPaths = new Set()

  for (const budget of config.pathBudgets) {
    const content = contents.get(budget.path)
    if (content === undefined) {
      errors.push(`缺少受预算保护的文件：${budget.path}`)
      continue
    }
    checkedPaths.add(budget.path)
    const actual = metrics(content)
    const limits = [
      ['maxLines', 'lines', '行数'],
      ['maxCharacters', 'characters', '字符数'],
      ['maxLineLength', 'maxLineLength', '最长单行字符数'],
      ['maxUseState', 'useState', 'useState 调用数'],
      ['maxUseEffect', 'useEffect', 'useEffect 调用数'],
      ['maxAsyncFunctions', 'asyncFunctions', 'async 声明数'],
      ['maxTopLevelProps', 'topLevelProps', '组件顶层 props 数'],
    ]
    for (const [limitKey, metricKey, label] of limits) {
      if (budget[limitKey] !== undefined && actual[metricKey] === null) {
        errors.push(`${budget.path} 的${label}无法解析。受 props 预算保护的组件必须使用 export function Name({ ... }: Props) 直接参数解构。`)
        continue
      }
      if (budget[limitKey] !== undefined && actual[metricKey] > budget[limitKey]) {
        errors.push(`${budget.path} 的${label} ${actual[metricKey]} 超过预算 ${budget[limitKey]}。请拆分职责，不要提高预算掩盖增长。`)
      }
    }
  }

  for (const budget of config.directoryBudgets) {
    for (const filePath of directoryFiles.get(budget.directory) ?? []) {
      const content = contents.get(filePath)
      if (content === undefined) continue
      checkedPaths.add(filePath)
      const actual = metrics(content)
      const limits = [
        ['maxLines', 'lines', '行数'],
        ['maxCharacters', 'characters', '字符数'],
        ['maxLineLength', 'maxLineLength', '最长单行字符数'],
      ]
      for (const [limitKey, metricKey, label] of limits) {
        if (budget[limitKey] !== undefined && actual[metricKey] > budget[limitKey]) {
          errors.push(`${filePath} 的${label} ${actual[metricKey]} 超过目录预算 ${budget[limitKey]}。请按领域或独立副作用拆分。`)
        }
      }
    }
  }

  for (const filePath of config.forbiddenPaths) {
    if (contents.has(filePath)) errors.push(`已废弃边界不得恢复：${filePath}`)
  }

  for (const replacement of config.budgetReplacements ?? []) {
    const targetBudget = config.pathBudgets.find((budget) => budget.path === replacement.to)
    if (!config.forbiddenPaths.includes(replacement.from)) {
      errors.push(`架构预算迁移必须把旧路径加入 forbiddenPaths：${replacement.from}`)
    }
    if (!targetBudget) {
      errors.push(`架构预算迁移缺少目标预算：${replacement.from} -> ${replacement.to}`)
    }
    if (!contents.has(replacement.to)) {
      errors.push(`架构预算迁移缺少目标文件：${replacement.from} -> ${replacement.to}`)
    }
  }

  for (const rule of config.forbiddenText) {
    const content = contents.get(rule.path)
    if (content?.includes(rule.text)) errors.push(`${rule.path}: ${rule.message}`)
  }

  return { checkedPaths: [...checkedPaths], errors }
}

export function evaluateBudgetRatchet(config, baseline, contents, directoryFiles = new Map()) {
  if (!baseline) return []
  const errors = []

  for (const previous of baseline.pathBudgets) {
    const current = config.pathBudgets.find((budget) => budget.path === previous.path)
    if (!current) {
      const replacement = (config.budgetReplacements ?? []).find((candidate) => candidate.from === previous.path)
      const replacementBudget = replacement
        ? config.pathBudgets.find((budget) => budget.path === replacement.to)
        : undefined
      const inheritedLimits = replacementBudget && numericLimits.every((key) => (
        previous[key] === undefined
        || (replacementBudget[key] !== undefined && replacementBudget[key] <= previous[key])
      ))
      const inheritedForbiddenText = replacement && (baseline.forbiddenText ?? [])
        .filter((rule) => rule.path === previous.path)
        .every((previousRule) => config.forbiddenText.some((rule) => (
          rule.path === replacement.to && rule.text === previousRule.text
        )))
      const validReplacement = replacement
        && replacement.to !== previous.path
        && inheritedLimits
        && inheritedForbiddenText
        && config.forbiddenPaths.includes(previous.path)
        && contents.has(replacement.to)
        && !contents.has(previous.path)
      if (!validReplacement) {
        errors.push(`不得移除架构预算：${previous.path}。迁移必须删除并禁用旧路径、声明永久 replacement，并让新目标继承全部数值上限与跨层禁止规则。`)
      }
      continue
    }
    for (const key of numericLimits) {
      if (previous[key] === undefined) continue
      if (current[key] === undefined || current[key] > previous[key]) {
        errors.push(`${previous.path} 的 ${key} 不得从 ${previous[key]} 放宽为 ${current[key] ?? '未限制'}。请拆分职责。`)
      }
    }
  }

  for (const previous of baseline.directoryBudgets) {
    const current = config.directoryBudgets.find((budget) => budget.directory === previous.directory)
    if (!current) {
      errors.push(`不得移除目录架构预算：${previous.directory}。空目录预算保留为防回归墓碑。`)
      continue
    }
    for (const key of ['maxLines', 'maxCharacters', 'maxLineLength']) {
      if (previous[key] === undefined) continue
      if (current[key] === undefined || current[key] > previous[key]) {
        errors.push(`${previous.directory} 的 ${key} 不得从 ${previous[key]} 放宽为 ${current[key] ?? '未限制'}。`)
      }
    }
    for (const extension of previous.extensions) {
      if (!current.extensions.includes(extension)) {
        errors.push(`${previous.directory} 不得移除受保护扩展名：${extension}`)
      }
    }
  }

  for (const previousPath of baseline.forbiddenPaths) {
    if (!config.forbiddenPaths.includes(previousPath)) {
      errors.push(`不得移除已废弃边界保护：${previousPath}`)
    }
  }

  for (const previousRule of baseline.forbiddenText) {
    const retained = config.forbiddenText.some((rule) => (
      rule.path === previousRule.path && rule.text === previousRule.text
    ))
    if (!retained) errors.push(`不得移除跨层调用保护：${previousRule.path} / ${previousRule.text}`)
  }

  for (const previous of baseline.budgetReplacements ?? []) {
    const retained = (config.budgetReplacements ?? []).some((replacement) => (
      replacement.from === previous.from && replacement.to === previous.to
    ))
    if (!retained) errors.push(`不得移除架构预算迁移记录：${previous.from} -> ${previous.to}`)
  }

  return errors
}

function main() {
  const options = parseArguments(process.argv.slice(2))
  const serializedConfig = readTarget(budgetPath, options.staged)
  if (!serializedConfig) throw new Error(`Cannot read ${budgetPath}.`)
  const config = JSON.parse(serializedConfig)
  let baseline
  try {
    baseline = JSON.parse(runGit(['show', `HEAD:${budgetPath}`]))
  } catch {
    baseline = undefined
  }
  const directoryFiles = new Map(config.directoryBudgets.map((budget) => [
    budget.directory,
    listDirectoryFiles(budget.directory, budget.extensions, options.staged),
  ]))
  const allPaths = new Set([
    ...config.pathBudgets.map((budget) => budget.path),
    ...config.forbiddenPaths,
    ...config.forbiddenText.map((rule) => rule.path),
    ...(config.budgetReplacements ?? []).flatMap((replacement) => [replacement.from, replacement.to]),
    ...(baseline?.pathBudgets.map((budget) => budget.path) ?? []),
    ...[...directoryFiles.values()].flat(),
  ])
  const contents = new Map()
  for (const filePath of allPaths) {
    const content = readTarget(filePath, options.staged)
    if (content !== undefined) contents.set(filePath, content)
  }
  const result = evaluateArchitecture(config, contents, directoryFiles)
  result.errors.push(...evaluateBudgetRatchet(config, baseline, contents, directoryFiles))
  if (result.errors.length > 0) {
    console.error('架构预算检查失败：')
    for (const error of result.errors) console.error(`- ${error}`)
    process.exitCode = 1
    return
  }
  console.log(`架构预算检查通过：${result.checkedPaths.length} 个文件。`)
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) main()
