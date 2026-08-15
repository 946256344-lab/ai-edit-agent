// 根据变更文件判断必须同步的长期文档和变更记录，防止契约修改只停留在代码里。
import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import process from 'node:process'

const root = process.cwd()

function normalizePath(filePath) {
  return filePath.replaceAll('\\', '/')
}

function globMatches(filePath, pattern) {
  let expression = ''

  for (let index = 0; index < pattern.length; index += 1) {
    if (pattern.slice(index, index + 3) === '**/') {
      expression += '(?:.*/)?'
      index += 2
    } else if (pattern.slice(index, index + 2) === '**') {
      expression += '.*'
      index += 1
    } else if (pattern[index] === '*') {
      expression += '[^/]*'
    } else {
      expression += pattern[index].replace(/[.+^${}()|[\]\\]/g, '\\$&')
    }
  }

  return new RegExp(`^${expression}$`).test(filePath)
}

function runGit(args) {
  return execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim()
}

function currentFiles() {
  const status = execFileSync('git', ['status', '--porcelain', '--untracked-files=all'], { cwd: root, encoding: 'utf8' })
  if (!status.trim()) {
    return []
  }

  return status.trimEnd().split('\n').map((line) => normalizePath(line.slice(3).replace(/^"|"$/g, '')))
}

function stagedFiles() {
  const output = runGit(['diff', '--cached', '--name-only', '--diff-filter=ACMRD'])
  return output ? output.split('\n').map(normalizePath) : []
}

function stagedFileContent(filePath) {
  try {
    return runGit(['show', `:${filePath}`])
  } catch {
    return undefined
  }
}

function parseArguments(args) {
  const options = { files: [], staged: false }

  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index]
    if (argument === '--files') {
      const value = args[index + 1]
      if (!value) {
        throw new Error('--files requires a comma-separated file list.')
      }
      options.files.push(...value.split(',').filter(Boolean).map(normalizePath))
      index += 1
    } else if (argument === '--staged') {
      options.staged = true
    } else {
      throw new Error(`Unknown argument: ${argument}`)
    }
  }

  return options
}

export function evaluateDocSync(files, policy, recordContents) {
  const changedFiles = [...new Set(files.map(normalizePath))]
  const triggeredRules = policy.rules.filter((rule) =>
    changedFiles.some((filePath) => rule.sources.some((pattern) => globMatches(filePath, pattern))),
  )

  if (triggeredRules.length === 0) {
    return { errors: [], triggeredRules: [] }
  }

  const requiredDocs = [...new Set(triggeredRules.flatMap((rule) => rule.requiredDocs))]
  const errors = requiredDocs
    .filter((documentPath) => !changedFiles.includes(documentPath))
    .map((documentPath) => `缺少必需的同步文档：${documentPath}`)

  const records = changedFiles.filter(
    (filePath) =>
      filePath.startsWith(`${policy.changeRecordDirectory}/`) &&
      filePath.endsWith('.md') &&
      !filePath.endsWith('/README.md'),
  )

  if (records.length === 0) {
    errors.push(`缺少架构变更记录：${policy.changeRecordDirectory}/YYYY-MM-DD-<主题>.md`)
  } else {
    const documentedFiles = new Set(records.flatMap((recordPath) => recordContents.get(recordPath)?.match(/(?:AGENTS\.md|README\.md|TASKS\.md|docs\/[\w/-]+\.md)/g) ?? []))
    for (const documentPath of requiredDocs) {
      if (!documentedFiles.has(documentPath)) {
        errors.push(`变更记录没有列出同步文档：${documentPath}`)
      }
    }
  }

  return { errors, triggeredRules: triggeredRules.map((rule) => rule.id) }
}

function main() {
  const options = parseArguments(process.argv.slice(2))
  const policy = JSON.parse(readFileSync(resolve(root, '.harness/doc-sync-policy.json'), 'utf8'))
  const files = options.files.length > 0 ? options.files : options.staged ? stagedFiles() : currentFiles()
  const recordContents = new Map()
  for (const filePath of files) {
    if (!filePath.startsWith(`${policy.changeRecordDirectory}/`) || !filePath.endsWith('.md')) {
      continue
    }

    const content = options.staged ? stagedFileContent(filePath) : readFileSync(resolve(root, filePath), 'utf8')
    if (content !== undefined) {
      recordContents.set(filePath, content)
    }
  }
  const result = evaluateDocSync(files, policy, recordContents)

  if (result.errors.length > 0) {
    console.error('文档同步检查失败：')
    for (const error of result.errors) {
      console.error(`- ${error}`)
    }
    console.error(`触发规则：${result.triggeredRules.join(', ')}`)
    process.exitCode = 1
    return
  }

  console.log(result.triggeredRules.length === 0 ? '未触发架构文档同步规则。' : `文档同步检查通过：${result.triggeredRules.join(', ')}`)
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  main()
}
