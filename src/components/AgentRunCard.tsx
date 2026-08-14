import { useEffect, useMemo, useState } from 'react'
import { listAgentRunSteps } from '../lib/local-store'
import type { StoredAgentRunStep, StoredAgentTask } from '../lib/local-store'

type AgentRunCardProps = {
  task: StoredAgentTask
  onOpenStoryboard: () => void
}

const ACTIVE_TASK_STATUSES = new Set<StoredAgentTask['status']>(['queued', 'running'])

const TOOL_LABELS: Record<string, string> = {
  agent_loop: '执行剪辑任务',
  list_assets: '检查可用素材',
  search_assets: '检索素材候选',
  search_asset_segments: '检索可用素材片段',
  request_asset_analysis: '准备素材分析',
  get_asset_health_summary: '读取素材健康状态',
  get_storyboard: '读取 storyboard',
  generate_storyboard: '生成 storyboard',
  get_timeline: '检查当前时间线',
  get_text_capabilities: '检查文本设计能力',
  create_timeline_draft: '创建内部时间线',
  replace_clips: '替换时间线片段',
  change_clip_duration: '调整片段时长',
  reorder_clips: '调整镜头顺序',
  replace_text_tracks: '设计文本轨',
  replace_music_tracks: '设计音乐轨',
  search_music: '搜索可用音乐',
  download_music: '下载选定音乐',
  use_online_music: '添加在线音乐',
  render_preview: '渲染 local preview',
  create_jianying_draft: '创建 Jianying draft',
  finish: '整理并回答',
  done: '整理并回答',
  no_action: '确认无需操作',
}

const ARTIFACT_LABELS: Record<string, string> = {
  storyboard_version: 'Storyboard',
  timeline_version: '内部时间线',
  preview: 'Local preview',
  jianying_draft: 'Jianying draft',
  asset_analysis: '素材分析任务',
}

function toolLabel(toolName: string) {
  return TOOL_LABELS[toolName] ?? '执行受限操作'
}

function taskStatusCopy(status: StoredAgentTask['status']) {
  if (status === 'completed') return '已完成'
  if (status === 'partially_completed') return '部分完成'
  if (status === 'needs_clarification') return '需要你的回答'
  if (status === 'needs_review') return '需要检查'
  if (status === 'failed' || status === 'cancelled') return '未能完成'
  if (status === 'running') return '正在执行'
  return '等待执行'
}

function stepStatusCopy(status: StoredAgentRunStep['status']) {
  if (status === 'completed') return '已完成'
  if (status === 'failed') return '尝试未成功'
  if (status === 'running') return '正在执行'
  return '等待执行'
}

function elapsedCopy(task: StoredAgentTask) {
  const end = ACTIVE_TASK_STATUSES.has(task.status) ? Date.now() : task.updatedAt
  const seconds = Math.max(0, Math.round((end - task.createdAt) / 1000))
  if (seconds < 60) return `${seconds} 秒`
  return `${Math.floor(seconds / 60)} 分 ${String(seconds % 60).padStart(2, '0')} 秒`
}

export function AgentRunCard({ task, onOpenStoryboard }: AgentRunCardProps) {
  const [steps, setSteps] = useState<StoredAgentRunStep[]>([])
  const [expanded, setExpanded] = useState(ACTIVE_TASK_STATUSES.has(task.status))
  const [, setClock] = useState(0)

  useEffect(() => {
    let active = true
    if (!task.editingTaskId) return () => { active = false }
    const refresh = () => void listAgentRunSteps(task.projectId, task.editingTaskId ?? '', task.id)
      .then((nextSteps) => {
        if (active) setSteps(nextSteps)
      })
      .catch(() => undefined)
    refresh()
    if (!ACTIVE_TASK_STATUSES.has(task.status)) return () => { active = false }
    const interval = window.setInterval(refresh, 800)
    return () => {
      active = false
      window.clearInterval(interval)
    }
  }, [task.editingTaskId, task.id, task.projectId, task.status])

  useEffect(() => {
    if (!ACTIVE_TASK_STATUSES.has(task.status)) return
    const interval = window.setInterval(() => setClock((value) => value + 1), 1000)
    return () => window.clearInterval(interval)
  }, [task.status])

  const sortedSteps = useMemo(
    () => [...steps].sort((left, right) => left.stepNumber - right.stepNumber),
    [steps],
  )
  const currentStep = sortedSteps.find((step) => step.status === 'running')
    ?? sortedSteps.find((step) => step.status === 'queued')
  const completedCount = sortedSteps.filter((step) => step.status === 'completed').length
  const artifacts = [...new Set(sortedSteps
    .filter((step) => step.status === 'completed' && step.artifactType)
    .map((step) => step.artifactType as string))]
  const currentCopy = currentStep
    ? toolLabel(currentStep.toolName)
    : ACTIVE_TASK_STATUSES.has(task.status)
      ? 'Agent 正在确定下一步'
      : taskStatusCopy(task.status)
  const hasStoryboard = artifacts.includes('storyboard_version')

  return <section className={`agent-run-card ${task.status}`} aria-live={ACTIVE_TASK_STATUSES.has(task.status) ? 'polite' : 'off'}>
    <button className="agent-run-summary" type="button" onClick={() => setExpanded((value) => !value)} aria-expanded={expanded}>
      <span className={`agent-run-state ${task.status}`} aria-hidden="true" />
      <span className="agent-run-copy">
        <strong>{currentCopy}</strong>
        <small>{taskStatusCopy(task.status)} · 已完成 {completedCount} 个步骤 · {elapsedCopy(task)}</small>
      </span>
      <span className="agent-run-toggle">{expanded ? '收起' : '查看步骤'}</span>
    </button>
    {expanded && <div className="agent-run-details">
      {sortedSteps.length > 0
        ? <ol className="agent-step-list">{sortedSteps.map((step) => <li className={step.status} key={step.id}>
          <span className="agent-step-icon" aria-hidden="true" />
          <span><strong>{toolLabel(step.toolName)}</strong><small>{stepStatusCopy(step.status)}</small></span>
        </li>)}</ol>
        : <p className="agent-run-waiting">任务已进入本地队列，正在等待第一个安全步骤。</p>}
      {artifacts.length > 0 && <div className="agent-run-artifacts"><strong>真实产物</strong><ul>{artifacts.map((artifact) => <li key={artifact}><span>✓</span>{ARTIFACT_LABELS[artifact] ?? '本地产物'}</li>)}</ul>{hasStoryboard && <button type="button" onClick={onOpenStoryboard}>查看 storyboard</button>}</div>}
      {(task.status === 'needs_clarification' || task.status === 'needs_review') && <p className="agent-run-attention">{task.status === 'needs_clarification' ? '请在对话中回答 Agent 提出的问题后继续。' : '上次执行意外中断，现有产物未自动重放，请检查后重新运行。'}</p>}
      {(task.status === 'failed' || task.status === 'cancelled') && <p className="agent-run-attention error">任务未完成；现有 storyboard、时间线和 preview 不会被自动覆盖。</p>}
    </div>}
  </section>
}
