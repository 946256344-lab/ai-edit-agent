# 2026-09-03：AI 默认少写新测试

## 变更

在协作流程中明确：编码 Agent **默认不写新测试**；仅契约/fixture、真实 bug 回归、或用户/审查明确要求时才补测。提交前仍跑已有适用验证，不主动扩测。

## 文件

- `CONTRIBUTING.md`：新增「AI 写测试」事实源
- `AGENTS.md`：一行指针
- `docs/codebase/TESTING.md`：§0 指向 CONTRIBUTING

## 验证

文档约定变更；无代码行为变化。
