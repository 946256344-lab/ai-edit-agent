import assert from 'node:assert/strict'
import { evaluateAgentContextRatchet, evaluateAgentContracts } from './check-agent-contracts.mjs'

const codebaseDocs = ['STACK.md', 'STRUCTURE.md', 'ARCHITECTURE.md', 'CONVENTIONS.md', 'INTEGRATIONS.md', 'TESTING.md', 'CONCERNS.md']
const config = {
  version: 1,
  stagedRuntimeFiles: ['.githooks/pre-commit'],
  stagedHook: {
    path: '.githooks/pre-commit',
    requiredCommands: ['node check.mjs --staged'],
  },
  requiredInstructionFiles: ['AGENTS.md', 'src/AGENTS.md', 'src-tauri/src/AGENTS.md'],
  codebaseDocs,
  taskWindow: {
    path: 'TASKS.md',
    start: '<!-- ACTIVE_TASKS_START -->',
    end: '<!-- ACTIVE_TASKS_END -->',
    maxNonEmptyLines: 2,
    maxCharacters: 200,
  },
  scopes: [
    { id: 'frontend', root: 'src', instructions: 'src/AGENTS.md', requiredDocs: ['docs/codebase/STRUCTURE.md'], verify: ['npm run build'] },
    { id: 'rust', root: 'src-tauri/src', instructions: 'src-tauri/src/AGENTS.md', requiredDocs: ['docs/codebase/ARCHITECTURE.md'], verify: ['cargo test'] },
  ],
  boundaries: {
    tauriInvoke: { roots: ['src'], extensions: ['.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs'], allowedPaths: ['src/lib/local-store.ts'] },
    windowsProcess: { roots: ['src-tauri/src'], extensions: ['.rs'], allowedPaths: ['src-tauri/src/process.rs'] },
    credentialAccess: { roots: ['src-tauri/src'], extensions: ['.rs'], allowedPaths: ['src-tauri/src/oauth.rs'] },
    httpAccess: { roots: ['src-tauri/src'], extensions: ['.rs'], allowedPaths: ['src-tauri/src/provider.rs'] },
  },
}

function validRepository() {
  return new Map([
    ['AGENTS.md', '# Root'],
    ['.githooks/pre-commit', 'node check.mjs --staged || exit 1'],
    ['src/AGENTS.md', '# Frontend'],
    ['src-tauri/src/AGENTS.md', '# Rust'],
    ...codebaseDocs.map((name) => [`docs/codebase/${name}`, `# ${name}\n\n## 证据\n\n- fixture`]),
    ['TASKS.md', '# Tasks\n<!-- ACTIVE_TASKS_START -->\n- [ ] one\n<!-- ACTIVE_TASKS_END -->'],
    ['src/lib/local-store.ts', "import { invoke } from '@tauri-apps/api/core'\ninvoke<void>('ping')"],
    ['src-tauri/src/lib.rs', 'tauri::generate_handler![commands::ping])'],
    ['docs/api.md', '| `ping` |'],
    ['src-tauri/src/process.rs', 'Command::new("ffmpeg")'],
    ['src-tauri/src/oauth.rs', 'keyring::Entry::new("a", "b")'],
    ['src-tauri/src/provider.rs', 'ureq::AgentBuilder::new()'],
    ['src-tauri/src/agentloop.rs', 'const OBSERVATION_TOOLS: &[&str] = &["observe"];\nconst EDIT_TOOLS: &[&str] = &["edit"];\nlet accepted = matches!(tool.as_str(), "ask_user" | "finish" | "done" | "no_action");'],
    ['src/lib/agent-tools.ts', "export type AgentObservationToolName = 'observe'\n\nexport type AgentSideEffectToolName = 'edit'\n\nexport type AgentControlToolName = 'ask_user' | 'finish'\n\nexport type AgentControlToolAlias = 'no_action' | 'done'"],
    ['src-tauri/tests/fixtures/agent_tool_contracts.v1.json', JSON.stringify({ tools: [{ name: 'observe', kind: 'observation' }, { name: 'edit', kind: 'edit' }], controlActions: [{ name: 'ask_user' }, { name: 'finish', aliases: ['no_action', 'done', 'empty tool'] }] })],
  ])
}

function errorsFor(mutator) {
  const repository = validRepository()
  mutator(repository)
  return evaluateAgentContracts(repository, config).errors.join('\n')
}

assert.deepEqual(evaluateAgentContracts(validRepository(), config).errors, [])
assert.match(errorsFor((repository) => repository.set('src/AGENTS.md', '')), /缺少 Agent 上下文文件/)
assert.match(errorsFor((repository) => repository.set('docs/codebase/NOTES.md', 'temporary')), /只能保留清单内七份文档/)
assert.match(errorsFor((repository) => repository.set('docs/codebase/notes/EXTRA.md', 'temporary')), /只能保留清单内七份文档/)
assert.match(errorsFor((repository) => repository.set('TASKS.md', '# Tasks\n<!-- ACTIVE_TASKS_START -->\n<!-- ACTIVE_TASKS_END -->')), /当前任务窗口不能为空/)
assert.match(errorsFor((repository) => repository.set('TASKS.md', '# Tasks\n<!-- ACTIVE_TASKS_START -->\na\nb\nc\n<!-- ACTIVE_TASKS_END -->')), /当前任务窗口超过/)
assert.match(errorsFor((repository) => repository.set('src/components/Bad.tsx', "invoke('ping')")), /Tauri invoke 只能/)
assert.match(errorsFor((repository) => repository.set('src/Bad.js', "import { invoke } from '@tauri-apps/api/core'\ninvoke('ping')")), /Tauri invoke 只能/)
assert.match(errorsFor((repository) => repository.set('src/components/Bad.tsx', "import { invoke as call } from '@tauri-apps/api/core'\ncall('ping')")), /Tauri invoke 只能/)
assert.match(errorsFor((repository) => repository.set('src/components/Bad.tsx', "const { invoke: call } = await import('@tauri-apps/api/core')\ncall('ping')")), /Tauri invoke 只能/)
assert.match(errorsFor((repository) => repository.set('src/components/Bad.tsx', "export { invoke as call } from '@tauri-apps/api/core'")), /Tauri invoke 只能/)
assert.match(errorsFor((repository) => repository.set('src/lib/local-store.ts', "invoke('missing')")), /未注册的 Tauri 命令/)
assert.match(errorsFor((repository) => repository.set('src/lib/local-store.ts', "import { invoke } from '@tauri-apps/api/core'\nconst command = 'ping'\ninvoke(command)")), /静态字符串命令名/)
assert.match(errorsFor((repository) => repository.set('src/lib/local-store.ts', "import { invoke } from '@tauri-apps/api/core'\nconst call = invoke\ncall('missing')")), /不得重命名、转存/)
assert.deepEqual(evaluateAgentContracts(new Map([...validRepository(), ['src-tauri/src/lib.rs', 'tauri::generate_handler![ping]']]), config).errors, [])
assert.match(errorsFor((repository) => repository.set('docs/api.md', 'Narrative mentions `ping` but has no command table.')), /未写入 docs\/api\.md/)
assert.match(errorsFor((repository) => repository.set('src/lib/agent-tools.ts', "export type AgentObservationToolName = 'other'")), /观察工具.*发生漂移/)
assert.match(errorsFor((repository) => repository.set('src-tauri/src/agentloop.rs', 'const OBSERVATION_TOOLS: &[&str] = &["observe"];\nconst EDIT_TOOLS: &[&str] = &["edit"];\nlet accepted = matches!(tool.as_str(), "ask_user" | "finish" | "later");')), /Rust 接受的控制动作/)
assert.match(errorsFor((repository) => repository.set('src-tauri/src/assets.rs', 'Command::new("ffmpeg")')), /Windows 外部进程创建.*只能/)
assert.match(errorsFor((repository) => repository.set('src-tauri/src/assets.rs', 'use std::process::Command as ProcessCommand;')), /Windows 外部进程创建.*只能/)
assert.match(errorsFor((repository) => repository.set('src-tauri/src/assets.rs', 'use std::{process::Command as Child};')), /Windows 外部进程创建.*只能/)
assert.match(errorsFor((repository) => repository.set('src-tauri/src/assets.rs', 'keyring::Entry::new("a", "b")')), /Credential Manager 访问.*只能/)
assert.match(errorsFor((repository) => repository.set('src-tauri/src/assets.rs', 'use ureq as web;')), /HTTP\/网络传输.*只能/)
assert.match(errorsFor((repository) => repository.set('src-tauri/src/assets.rs', 'use std::{net::TcpStream};')), /HTTP\/网络传输.*只能/)

function changedConfig(mutator) {
  const next = structuredClone(config)
  mutator(next)
  return evaluateAgentContextRatchet(next, config).join('\n')
}

assert.deepEqual(evaluateAgentContextRatchet(config, undefined), [])
assert.match(changedConfig((next) => { next.taskWindow.maxCharacters += 1 }), /不得放宽当前任务窗口/)
assert.match(changedConfig((next) => { next.boundaries.tauriInvoke.allowedPaths.push('src/Bad.tsx') }), /不得扩大 tauriInvoke 允许路径/)
assert.match(changedConfig((next) => { next.boundaries.windowsProcess.extensions = [] }), /不得移除 windowsProcess 受检扩展名/)
assert.match(changedConfig((next) => { next.requiredInstructionFiles = [] }), /不得移除既有 Agent 指令/)
assert.match(changedConfig((next) => { next.stagedHook.requiredCommands = [] }), /不得移除提交钩子命令/)

console.log('Agent 契约检查单元测试通过。')
