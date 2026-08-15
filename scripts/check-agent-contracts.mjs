// 校验 Agent 上下文入口、IPC/进程/网络所有权、工具目录与文档结构没有漂移。
import { execFileSync } from 'node:child_process'
import { extname, resolve } from 'node:path'
import { readFileSync } from 'node:fs'
import process from 'node:process'

const root = process.cwd()

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

function repositoryFiles(staged) {
  const output = runGit(staged
    ? ['ls-files', '--cached']
    : ['ls-files', '--cached', '--others', '--exclude-standard'])
  return output ? output.split('\n').map(normalizePath) : []
}

function stagedFileContent(filePath) {
  try {
    return execFileSync('git', ['show', `:${filePath}`], { cwd: root, encoding: 'utf8' })
  } catch {
    return undefined
  }
}

function loadRepository(staged) {
  const files = repositoryFiles(staged)
  const relevantExtensions = new Set(['.md', '.json', '.js', '.jsx', '.mjs', '.cjs', '.ts', '.tsx', '.rs', '.py', '.css', '.html'])
  const contents = new Map()
  for (const filePath of files) {
    if (!relevantExtensions.has(extname(filePath)) && !filePath.startsWith('docs/codebase/') && filePath !== '.githooks/pre-commit') {
      continue
    }
    const content = staged ? stagedFileContent(filePath) : readFileSync(resolve(root, filePath), 'utf8')
    if (content !== undefined) {
      contents.set(filePath, content)
    }
  }
  return contents
}

function occurrences(content, needle) {
  return content.split(needle).length - 1
}

function literalValues(content, typeName) {
  const match = content.match(new RegExp(`export type ${typeName}\\s*=([\\s\\S]*?)(?=\\n\\s*(?:export type|/\\*\\*|export function)|$)`))
  return match ? [...match[1].matchAll(/['"]([^'"]+)['"]/g)].map((item) => item[1]) : []
}

function rustArray(content, constantName) {
  const match = content.match(new RegExp(`const ${constantName}:[^=]+=\\s*&\\[([\\s\\S]*?)\\];`))
  return match ? [...match[1].matchAll(/"([^"]+)"/g)].map((item) => item[1]) : []
}

function sortedUnique(values) {
  return [...new Set(values)].sort()
}

function sameValues(left, right) {
  return JSON.stringify(sortedUnique(left)) === JSON.stringify(sortedUnique(right))
}

function hasChineseNavigation(filePath, content, maxHeadLines) {
  const head = content.replaceAll('\r\n', '\n').split('\n').slice(0, maxHeadLines).join('\n')
  const chinese = '[\\u3400-\\u9fff]'
  if (filePath.endsWith('.py')) {
    return new RegExp(`(?:^\\s*#|\"\"\"|''')[\\s\\S]{0,1200}${chinese}`, 'm').test(head)
  }
  if (filePath.endsWith('.html')) {
    return new RegExp(`<!--[\\s\\S]{0,1200}${chinese}`).test(head)
  }
  if (filePath === '.githooks/pre-commit') {
    return new RegExp(`^\\s*#[^!\\n]*${chinese}`, 'm').test(head)
  }
  return new RegExp(`^\\s*(?://[/!]?|/\\*+|\\*)[^\\n]*${chinese}`, 'm').test(head)
}

function registeredCommands(content) {
  const block = content.match(/generate_handler!\s*\[([\s\S]*?)\]/)?.[1]
  if (block === undefined) {
    return { commands: [], valid: false }
  }
  const commands = []
  for (const entry of block.split(',').map((value) => value.trim()).filter(Boolean)) {
    const match = entry.match(/^(?:[A-Za-z_]\w*::)*([A-Za-z_]\w*)$/)
    if (!match) {
      return { commands: [], valid: false }
    }
    commands.push(match[1])
  }
  return { commands: sortedUnique(commands), valid: commands.length > 0 }
}

function invokedCommands(content) {
  return [...content.matchAll(/\binvoke(?:<[^>\n]+>)?\s*\(\s*['"]([^'"]+)['"]/g)].map((item) => item[1])
}

function runtimeControlNames(content) {
  const bodies = [...content.matchAll(/matches!\s*\(\s*tool\.as_str\(\)\s*,([\s\S]*?)\)/g)].map((match) => match[1])
  const controlBody = bodies.find((body) => body.includes('"ask_user"')) ?? ''
  return sortedUnique([...controlBody.matchAll(/"([^"]+)"/g)].map((item) => item[1]))
}

function checkBoundary(contents, boundary, pattern, label, errors) {
  for (const [filePath, content] of contents) {
    const inBoundaryRoot = boundary.roots.some((directory) => filePath === directory || filePath.startsWith(`${directory}/`))
    if (!inBoundaryRoot || !boundary.extensions.includes(extname(filePath)) || boundary.allowedPaths.includes(filePath)) {
      continue
    }
    if (pattern.test(content)) {
      errors.push(`${label} 只能出现在 ${boundary.allowedPaths.join('、')}，发现：${filePath}`)
    }
  }
}

export function evaluateAgentContracts(contents, config) {
  const errors = []
  const requiredFiles = new Set([
    ...config.stagedRuntimeFiles,
    config.stagedHook.path,
    ...config.requiredInstructionFiles,
    ...config.scopes.flatMap((scope) => [scope.instructions, ...scope.requiredDocs]),
  ])
  for (const filePath of requiredFiles) {
    if (!contents.get(filePath)?.trim()) {
      errors.push(`缺少 Agent 上下文文件：${filePath}`)
    }
  }
  const stagedHook = contents.get(config.stagedHook.path) ?? ''
  for (const command of config.stagedHook.requiredCommands) {
    if (!stagedHook.split(/\r?\n/).some((line) => line.trim() === `${command} || exit 1`)) {
      errors.push(`提交钩子缺少强制命令：${command}`)
    }
  }

  const expectedCodebaseDocs = config.codebaseDocs.map((name) => `docs/codebase/${name}`).sort()
  const actualCodebaseFiles = [...contents.keys()]
    .filter((filePath) => filePath.startsWith('docs/codebase/'))
    .sort()
  for (const filePath of expectedCodebaseDocs.filter((filePath) => !actualCodebaseFiles.includes(filePath))) {
    errors.push(`缺少代码库地图文档：${filePath}`)
  }
  for (const filePath of actualCodebaseFiles.filter((filePath) => !expectedCodebaseDocs.includes(filePath))) {
    errors.push(`docs/codebase 只能保留清单内七份文档，发现：${filePath}`)
  }
  for (const filePath of expectedCodebaseDocs) {
    const content = contents.get(filePath)
    if (content !== undefined && !/^## .*证据/m.test(content)) {
      errors.push(`代码库地图缺少证据章节：${filePath}`)
    }
  }

  const taskContent = contents.get(config.taskWindow.path)
  if (taskContent === undefined) {
    errors.push(`缺少当前任务文件：${config.taskWindow.path}`)
  } else {
    const startCount = occurrences(taskContent, config.taskWindow.start)
    const endCount = occurrences(taskContent, config.taskWindow.end)
    if (startCount !== 1 || endCount !== 1) {
      errors.push(`当前任务窗口标记必须各出现一次：${config.taskWindow.path}`)
    } else {
      const start = taskContent.indexOf(config.taskWindow.start) + config.taskWindow.start.length
      const end = taskContent.indexOf(config.taskWindow.end)
      if (start >= end) {
        errors.push(`当前任务窗口标记顺序错误：${config.taskWindow.path}`)
      } else {
        const activeLines = taskContent.slice(start, end).split(/\r?\n/).filter((line) => line.trim())
        if (activeLines.length === 0) {
          errors.push(`当前任务窗口不能为空：${config.taskWindow.path}`)
        }
        if (activeLines.length > config.taskWindow.maxNonEmptyLines) {
          errors.push(`当前任务窗口超过 ${config.taskWindow.maxNonEmptyLines} 行：${config.taskWindow.path}`)
        }
        if (taskContent.slice(start, end).length > config.taskWindow.maxCharacters) {
          errors.push(`当前任务窗口超过 ${config.taskWindow.maxCharacters} 字符：${config.taskWindow.path}`)
        }
      }
    }
  }

  const navigation = config.sourceNavigation
  if (navigation) {
    for (const [filePath, content] of contents) {
      const inRoot = navigation.roots.some((rootPath) => filePath.startsWith(`${rootPath}/`))
      const isExactFile = navigation.files.includes(filePath)
      if (((inRoot && navigation.extensions.includes(extname(filePath))) || isExactFile) && !hasChineseNavigation(filePath, content, navigation.maxHeadLines)) {
        errors.push(`手写源码缺少文件顶部中文职责导航：${filePath}`)
      }
    }
  }

  checkBoundary(contents, config.boundaries.tauriInvoke, /\binvoke\s*(?:<|\()|(?:import|export)\s*\{[^}]*\binvoke\b[^}]*\}\s*from\s*['"]@tauri-apps\/api\/core|import\s*\*\s*as\s+\w+\s+from\s*['"]@tauri-apps\/api\/core|import\s*\(\s*['"]@tauri-apps\/api\/core|__TAURI_INTERNALS__/, 'Tauri invoke', errors)
  checkBoundary(contents, config.boundaries.windowsProcess, /\bCommand::new\s*\(|(?:std|tokio)::(?:process\b|\{[^}]*\bprocess\s*::)/, 'Windows 外部进程创建', errors)
  checkBoundary(contents, config.boundaries.credentialAccess, /\bkeyring::|\bEntry::new\s*\(/, 'Credential Manager 访问', errors)
  checkBoundary(contents, config.boundaries.httpAccess, /\b(?:ureq|reqwest|hyper|isahc|attohttpc|surf)\b|\b(?:std|tokio)::(?:net\b|\{[^}]*\bnet\s*::)/, 'HTTP/网络传输', errors)

  const bridge = contents.get('src/lib/local-store.ts') ?? ''
  const tauriEntry = contents.get('src-tauri/src/lib.rs') ?? ''
  const api = contents.get('docs/api.md') ?? ''
  if (!/import\s*\{\s*invoke\s*\}\s*from\s*['"]@tauri-apps\/api\/core['"]/.test(bridge)) {
    errors.push('src/lib/local-store.ts 必须以未重命名的 invoke 作为唯一 IPC 入口。')
  }
  const invokeCalls = [...bridge.matchAll(/\binvoke(?:<[^>\n]+>)?\s*\(/g)]
  const literalInvocations = invokedCommands(bridge)
  if (invokeCalls.length !== literalInvocations.length) {
    errors.push('src/lib/local-store.ts 的每次 invoke 都必须使用静态字符串命令名。')
  }
  const invokeReferences = [...bridge.matchAll(/\binvoke\b/g)]
  if (invokeReferences.length !== invokeCalls.length + 1) {
    errors.push('src/lib/local-store.ts 不得重命名、转存或间接调用 invoke。')
  }
  const registration = registeredCommands(tauriEntry)
  const registered = registration.commands
  if (!registration.valid) {
    errors.push('无法从 src-tauri/src/lib.rs 解析 Tauri 命令注册表。')
  }
  for (const command of sortedUnique(literalInvocations).filter((command) => !registered.includes(command))) {
    errors.push(`前端调用了未注册的 Tauri 命令：${command}`)
  }
  const apiTableRows = api.split(/\r?\n/).filter((line) => line.trimStart().startsWith('|'))
  for (const command of registered.filter((command) => !apiTableRows.some((line) => line.includes(`\`${command}\``)))) {
    errors.push(`公开 Tauri 命令未写入 docs/api.md：${command}`)
  }

  // 工具授权事实必须留在纯策略模块；父循环仍负责解析 canonical control 及兼容别名。
  const rustPolicy = contents.get('src-tauri/src/agentloop/policy.rs') ?? ''
  const rustLoop = contents.get('src-tauri/src/agentloop.rs') ?? ''
  const typeScriptTools = contents.get('src/lib/agent-tools.ts') ?? ''
  let fixture
  try {
    fixture = JSON.parse(contents.get('src-tauri/tests/fixtures/agent_tool_contracts.v1.json') ?? '{}')
  } catch {
    errors.push('Agent 工具 fixture 不是有效 JSON。')
    fixture = {}
  }
  const fixtureTools = Array.isArray(fixture.tools) ? fixture.tools : []
  const fixtureControls = Array.isArray(fixture.controlActions) ? fixture.controlActions : []
  const checks = [
    ['观察工具', rustArray(rustPolicy, 'OBSERVATION_TOOLS'), literalValues(typeScriptTools, 'AgentObservationToolName'), fixtureTools.filter((tool) => tool.kind === 'observation').map((tool) => tool.name)],
    ['编辑/交付工具', rustArray(rustPolicy, 'EDIT_TOOLS'), literalValues(typeScriptTools, 'AgentSideEffectToolName'), fixtureTools.filter((tool) => tool.kind !== 'observation').map((tool) => tool.name)],
    ['控制动作', fixtureControls.map((tool) => tool.name), literalValues(typeScriptTools, 'AgentControlToolName'), fixtureControls.map((tool) => tool.name)],
    ['控制动作别名', fixtureControls.flatMap((tool) => tool.aliases ?? []).filter((name) => name !== 'empty tool'), literalValues(typeScriptTools, 'AgentControlToolAlias'), fixtureControls.flatMap((tool) => tool.aliases ?? []).filter((name) => name !== 'empty tool')],
  ]
  for (const [label, runtime, mirror, contract] of checks) {
    if (!sameValues(runtime, mirror) || !sameValues(runtime, contract)) {
      errors.push(`${label}在 Rust、TypeScript 与版本化 fixture 之间发生漂移。`)
    }
  }
  const runtimeControls = runtimeControlNames(`${rustPolicy}\n${rustLoop}`)
  const contractControls = fixtureControls.flatMap((tool) => [tool.name, ...(tool.aliases ?? [])]).filter((name) => name !== 'empty tool')
  if (!sameValues(runtimeControls, contractControls)) {
    errors.push('Rust 接受的控制动作与 TypeScript/版本化 fixture 发生漂移。')
  }

  return { errors }
}

function missingValues(current, previous) {
  const currentValues = Array.isArray(current) ? current : []
  const previousValues = Array.isArray(previous) ? previous : []
  return previousValues.filter((value) => !currentValues.includes(value))
}

export function evaluateAgentContextRatchet(config, baseline) {
  if (!baseline) {
    return []
  }
  const errors = []
  if (!Number.isInteger(config.version) || config.version < baseline.version) {
    errors.push('不得降低 Agent 上下文清单版本。')
  }
  for (const value of missingValues(config.requiredInstructionFiles, baseline.requiredInstructionFiles)) {
    errors.push(`不得移除既有 Agent 指令：${value}`)
  }
  if (!sameValues(config.codebaseDocs, baseline.codebaseDocs)) {
    errors.push('固定七份代码库地图清单不得增删。')
  }
  for (const field of ['path', 'start', 'end']) {
    if (config.taskWindow[field] !== baseline.taskWindow[field]) {
      errors.push(`不得修改当前任务窗口 ${field}。`)
    }
  }
  for (const field of ['maxNonEmptyLines', 'maxCharacters']) {
    if (!Number.isInteger(config.taskWindow[field]) || config.taskWindow[field] > baseline.taskWindow[field]) {
      errors.push(`不得放宽当前任务窗口 ${field}。`)
    }
  }
  if (baseline.sourceNavigation) {
    if (!config.sourceNavigation) {
      errors.push('不得移除中文源码导航门。')
    } else {
      for (const field of ['roots', 'files', 'extensions']) {
        for (const value of missingValues(config.sourceNavigation[field], baseline.sourceNavigation[field])) {
          errors.push(`不得缩小中文源码导航 ${field}：${value}`)
        }
      }
      if (!Number.isInteger(config.sourceNavigation.maxHeadLines) || config.sourceNavigation.maxHeadLines > baseline.sourceNavigation.maxHeadLines) {
        errors.push('不得放宽中文源码导航最大头部行数。')
      }
    }
  }
  for (const previousScope of baseline.scopes ?? []) {
    const currentScope = (config.scopes ?? []).find((scope) => scope.id === previousScope.id)
    if (!currentScope) {
      errors.push(`不得移除 Agent 作用域：${previousScope.id}`)
      continue
    }
    for (const field of ['root', 'instructions']) {
      if (currentScope[field] !== previousScope[field]) {
        errors.push(`不得修改 Agent 作用域 ${previousScope.id} 的 ${field}。`)
      }
    }
    for (const value of missingValues(currentScope.requiredDocs, previousScope.requiredDocs)) {
      errors.push(`不得移除 ${previousScope.id} 必读文档：${value}`)
    }
    for (const value of missingValues(currentScope.verify, previousScope.verify)) {
      errors.push(`不得移除 ${previousScope.id} 验证命令：${value}`)
    }
  }
  for (const [name, previousBoundary] of Object.entries(baseline.boundaries ?? {})) {
    const currentBoundary = config.boundaries?.[name]
    if (!currentBoundary) {
      errors.push(`不得移除可信边界：${name}`)
      continue
    }
    for (const value of missingValues(currentBoundary.extensions, previousBoundary.extensions)) {
      errors.push(`不得移除 ${name} 受检扩展名：${value}`)
    }
    for (const value of missingValues(currentBoundary.roots, previousBoundary.roots)) {
      errors.push(`不得移除 ${name} 受检目录：${value}`)
    }
    for (const value of currentBoundary.allowedPaths.filter((value) => !previousBoundary.allowedPaths.includes(value))) {
      errors.push(`不得扩大 ${name} 允许路径：${value}`)
    }
  }
  for (const value of missingValues(config.stagedRuntimeFiles, baseline.stagedRuntimeFiles)) {
    errors.push(`不得移除暂存检查运行文件：${value}`)
  }
  if (config.stagedHook.path !== baseline.stagedHook.path) {
    errors.push('不得更换提交钩子路径。')
  }
  for (const value of missingValues(config.stagedHook.requiredCommands, baseline.stagedHook.requiredCommands)) {
    errors.push(`不得移除提交钩子命令：${value}`)
  }
  return errors
}

function parseArguments(args) {
  if (args.length === 0) {
    return { staged: false }
  }
  if (args.length === 1 && args[0] === '--staged') {
    return { staged: true }
  }
  throw new Error(`Unknown arguments: ${args.join(' ')}`)
}

function main() {
  const options = parseArguments(process.argv.slice(2))
  const contents = loadRepository(options.staged)
  const configContent = contents.get('.harness/agent-context.json')
  if (!configContent) {
    console.error('Agent 契约检查失败：缺少 .harness/agent-context.json')
    process.exitCode = 1
    return
  }
  const config = JSON.parse(configContent)
  const result = evaluateAgentContracts(contents, config)
  let baseline
  try {
    baseline = JSON.parse(runGit(['show', 'HEAD:.harness/agent-context.json']))
  } catch {
    baseline = undefined
  }
  result.errors.push(...evaluateAgentContextRatchet(config, baseline))
  if (options.staged) {
    for (const filePath of config.stagedRuntimeFiles) {
      let worktree
      try {
        worktree = readFileSync(resolve(root, filePath), 'utf8').replaceAll('\r\n', '\n')
      } catch {
        worktree = undefined
      }
      const staged = contents.get(filePath)?.replaceAll('\r\n', '\n')
      if (worktree === undefined || staged === undefined || worktree !== staged) {
        result.errors.push(`暂存检查运行文件必须完整暂存：${filePath}`)
      }
    }
  }
  if (result.errors.length > 0) {
    console.error('Agent 契约检查失败：')
    for (const error of result.errors) {
      console.error(`- ${error}`)
    }
    process.exitCode = 1
    return
  }
  console.log('Agent 上下文、IPC、可信边界与工具目录检查通过。')
}

if (process.argv[1] && resolve(process.argv[1]) === resolve(import.meta.filename)) {
  main()
}
