// 把仓库内受版本控制的 pre-commit 门禁安装为当前仓库的 Git hooks 路径。
import { execFileSync } from 'node:child_process'

execFileSync('git', ['config', 'core.hooksPath', '.githooks'], { stdio: 'inherit' })
console.log('已启用版本控制的 Git hooks。')
