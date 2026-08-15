// 用合成仓库验证架构预算检查器会对增长、删除预算和解析失败保持封闭。
import assert from 'node:assert/strict'
import { evaluateArchitecture, evaluateBudgetRatchet } from './check-architecture-budgets.mjs'

const config = {
  pathBudgets: [{
    path: 'src/App.tsx',
    maxLines: 3,
    maxCharacters: 100,
    maxLineLength: 50,
    maxUseState: 1,
    maxAsyncFunctions: 1,
    maxTopLevelProps: 2,
  }],
  directoryBudgets: [{
    directory: 'src/components',
    extensions: ['.tsx'],
    maxLines: 2,
    maxCharacters: 80,
    maxLineLength: 60,
  }],
  forbiddenPaths: ['src/components/Legacy.tsx'],
  forbiddenText: [{ path: 'src/App.tsx', text: 'legacyCall', message: '旧调用不得恢复。' }],
  budgetReplacements: [],
}

const passing = evaluateArchitecture(
  config,
  new Map([
    ['src/App.tsx', 'export function App({ model, actions }: Props) {\n  const run = async () => {}\n}\n'],
    ['src/components/Panel.tsx', 'export function Panel({ model, actions }: Props) {}\n'],
  ]),
  new Map([['src/components', ['src/components/Panel.tsx']]]),
)
assert.deepEqual(passing.errors, [])

const failing = evaluateArchitecture(
  config,
  new Map([
    ['src/App.tsx', 'const [one] = useState(0)\nconst [two] = useState(0)\nlegacyCall()\n'],
    ['src/components/Panel.tsx', 'one\ntwo\nthree\n'],
    ['src/components/Legacy.tsx', 'legacy\n'],
  ]),
  new Map([['src/components', ['src/components/Panel.tsx', 'src/components/Legacy.tsx']]]),
)
assert.match(failing.errors.join('\n'), /行数/)
assert.match(failing.errors.join('\n'), /useState/)
assert.match(failing.errors.join('\n'), /旧调用不得恢复/)
assert.match(failing.errors.join('\n'), /已废弃边界不得恢复/)

const opaqueProps = evaluateArchitecture(
  config,
  new Map([['src/App.tsx', 'export function App(props: Props) {}\n']]),
)
assert.match(opaqueProps.errors.join('\n'), /组件顶层 props 数无法解析/)

const restProps = evaluateArchitecture(
  config,
  new Map([['src/App.tsx', 'export function App({ ...props }: Props) {}\n']]),
)
assert.match(restProps.errors.join('\n'), /组件顶层 props 数无法解析/)

const asyncArrowOverflow = evaluateArchitecture(
  config,
  new Map([['src/App.tsx', 'export function App({}: Props) {\nconst one = async () => {}\nconst two = async () => {}\n}\n']]),
)
assert.match(asyncArrowOverflow.errors.join('\n'), /async 声明数 2 超过预算 1/)

const compressedLine = evaluateArchitecture(
  config,
  new Map([['src/App.tsx', `export function App({}: Props) { ${'x'.repeat(70)} }\n`]]),
)
assert.match(compressedLine.errors.join('\n'), /最长单行字符数/)

const weakened = structuredClone(config)
weakened.pathBudgets[0].maxLines = 4
weakened.directoryBudgets[0].maxLines = 3
delete weakened.pathBudgets[0].maxCharacters
weakened.directoryBudgets[0].maxLineLength = 61
weakened.forbiddenText = []
const ratchetErrors = evaluateBudgetRatchet(
  weakened,
  config,
  new Map([['src/App.tsx', 'current\n']]),
  new Map([['src/components', ['src/components/Panel.tsx']]]),
)
assert.match(ratchetErrors.join('\n'), /不得从 2 放宽为 3/)
assert.match(ratchetErrors.join('\n'), /maxCharacters.*未限制/)
assert.match(ratchetErrors.join('\n'), /maxLineLength.*61/)
assert.match(ratchetErrors.join('\n'), /不得移除跨层调用保护/)

const removedBudget = structuredClone(config)
removedBudget.pathBudgets = []
const removedBudgetErrors = evaluateBudgetRatchet(removedBudget, config, new Map())
assert.match(removedBudgetErrors.join('\n'), /不得移除架构预算：src\/App.tsx/)

const replacementContents = new Map([['src/Application.tsx', 'export function Application() {}\n']])
const weakLimitsReplacement = structuredClone(config)
weakLimitsReplacement.pathBudgets = [{ path: 'src/Application.tsx', maxLines: 3 }]
weakLimitsReplacement.forbiddenPaths.push('src/App.tsx')
weakLimitsReplacement.forbiddenText.push({ ...config.forbiddenText[0], path: 'src/Application.tsx' })
weakLimitsReplacement.budgetReplacements = [{ from: 'src/App.tsx', to: 'src/Application.tsx' }]
const weakLimitsErrors = evaluateBudgetRatchet(weakLimitsReplacement, config, replacementContents)
assert.match(weakLimitsErrors.join('\n'), /继承全部数值上限与跨层禁止规则/)

const weakForbiddenReplacement = structuredClone(config)
weakForbiddenReplacement.pathBudgets = [{ ...config.pathBudgets[0], path: 'src/Application.tsx' }]
weakForbiddenReplacement.forbiddenPaths.push('src/App.tsx')
weakForbiddenReplacement.budgetReplacements = [{ from: 'src/App.tsx', to: 'src/Application.tsx' }]
const weakForbiddenErrors = evaluateBudgetRatchet(weakForbiddenReplacement, config, replacementContents)
assert.match(weakForbiddenErrors.join('\n'), /继承全部数值上限与跨层禁止规则/)

const replacedBudget = structuredClone(config)
replacedBudget.pathBudgets = [{ ...config.pathBudgets[0], path: 'src/Application.tsx' }]
replacedBudget.forbiddenPaths.push('src/App.tsx')
replacedBudget.forbiddenText.push({ ...config.forbiddenText[0], path: 'src/Application.tsx' })
replacedBudget.budgetReplacements = [{ from: 'src/App.tsx', to: 'src/Application.tsx' }]
replacementContents.set('src/Application.tsx', 'export function Application({}: Props) {}\n')
assert.deepEqual(evaluateArchitecture(replacedBudget, replacementContents).errors, [])
assert.deepEqual(evaluateBudgetRatchet(replacedBudget, config, replacementContents), [])

console.log('架构预算检查单元测试通过。')
