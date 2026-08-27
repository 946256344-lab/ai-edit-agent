// 用合成仓库验证架构预算检查器仍会对增长和解析失败保持封闭，且允许主动放宽。
import assert from 'node:assert/strict'
import { evaluateArchitecture, evaluateBudgetRatchet } from './check-architecture-budgets.mjs'

const config = {
  pathBudgets: [{
    path: 'src/App.tsx',
    maxUseState: 1,
    maxAsyncFunctions: 1,
    maxTopLevelProps: 2,
  }],
  directoryBudgets: [],
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
  new Map(),
)
assert.deepEqual(passing.errors, [])

const failing = evaluateArchitecture(
  config,
  new Map([
    ['src/App.tsx', 'const [one] = useState(0)\nconst [two] = useState(0)\nlegacyCall()\n'],
    ['src/components/Legacy.tsx', 'legacy\n'],
  ]),
  new Map(),
)
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

const removedBudget = structuredClone(config)
removedBudget.pathBudgets = []
assert.deepEqual(evaluateBudgetRatchet(removedBudget, config, new Map()), [])

console.log('架构预算检查单元测试通过。')
