// 展示单个 Agent task 的状态和运行步骤；步骤文案面向用户，不暴露内部工具名。
import { useEffect, useMemo, useState } from 'react'
import { listAgentRunSteps } from '../lib/local-store'
import type { StoredAgentRunStep, StoredAgentTask } from '../lib/local-store'
import { WorkspaceIcon } from './WorkspaceIcon'
import { useI18n } from '../lib/i18n'
import type { Messages } from '../lib/i18n'

type AgentRunCardProps = {
  task: StoredAgentTask
  onOpenStoryboard: () => void
}

const ACTIVE_TASK_STATUSES = new Set<StoredAgentTask['status']>(['queued', 'running'])

type StageId = 'analyze' | 'select' | 'edit' | 'preview' | 'deliver' | 'finish'

const TOOL_STAGES: Record<string, StageId> = {
  agent_loop: 'finish',
  read_logs: 'analyze',
  list_assets: 'analyze',
  search_assets: 'analyze',
  search_asset_segments: 'analyze',
  request_asset_analysis: 'analyze',
  retry_failed_asset_analysis: 'analyze',
  get_asset_health_summary: 'analyze',
  get_storyboard: 'select',
  generate_storyboard: 'select',
  get_timeline: 'edit',
  get_text_capabilities: 'edit',
  create_timeline_draft: 'edit',
  replace_clips: 'edit',
  insert_clips: 'edit',
  change_clip_duration: 'edit',
  reorder_clips: 'edit',
  replace_text_tracks: 'edit',
  replace_music_tracks: 'edit',
  search_music: 'edit',
  download_music: 'edit',
  use_online_music: 'edit',
  list_voices: 'edit',
  synthesize_voiceover: 'edit',
  render_preview: 'preview',
  create_jianying_draft: 'deliver',
  finish: 'finish',
  done: 'finish',
  no_action: 'finish',
}

function toolStage(toolName: string): StageId {
  return TOOL_STAGES[toolName] ?? 'edit'
}

function elapsedCopy(task: StoredAgentTask, t: Messages) {
  const end = ACTIVE_TASK_STATUSES.has(task.status) ? Date.now() : task.updatedAt
  const seconds = Math.max(0, Math.round((end - task.createdAt) / 1000))
  if (seconds < 60) return t.agentRun.elapsedSeconds(seconds)
  return t.agentRun.elapsedMinutes(Math.floor(seconds / 60), String(seconds % 60).padStart(2, '0'))
}

export function AgentRunCard({ task, onOpenStoryboard }: AgentRunCardProps) {
  const { t } = useI18n()
  const copy = t.agentRun
  const [steps, setSteps] = useState<StoredAgentRunStep[]>([])
  const [expanded, setExpanded] = useState(false)
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
    ? copy.stages[toolStage(currentStep.toolName)]
    : ACTIVE_TASK_STATUSES.has(task.status)
      ? copy.preparingNext
      : task.status === 'completed'
        ? copy.done
        : copy.taskStatus[task.status]
  const hasResult = artifacts.includes('storyboard_version')
    || artifacts.includes('timeline_version')
    || artifacts.includes('preview')

  // 平时只露一行状态；步骤明细点开才显示。需要用户处理的提示始终可见。
  return <section className={`agent-run ${task.status}`} aria-live={ACTIVE_TASK_STATUSES.has(task.status) ? 'polite' : 'off'}>
    <div className="agent-run-line">
      <button className="agent-run-toggle" type="button" onClick={() => setExpanded((value) => !value)} aria-expanded={expanded} title={expanded ? copy.collapse : copy.viewSteps}>
        <span className={`agent-run-state ${task.status}`} aria-hidden="true" />
        <strong>{currentCopy}</strong>
        <small>{elapsedCopy(task, t)} · {copy.stepsDone(completedCount)}</small>
        <WorkspaceIcon name="chevron" />
      </button>
      {hasResult && !ACTIVE_TASK_STATUSES.has(task.status) && <button type="button" className="text-button agent-run-result" onClick={onOpenStoryboard}>{copy.viewResult}</button>}
    </div>
    {expanded && <div className="agent-run-details">
      {sortedSteps.length > 0
        ? <ol className="agent-run-steps">{sortedSteps.map((step) => <li className={step.status} key={step.id}>
          <span>{copy.tools[step.toolName] ?? copy.toolFallback}</span><small>{copy.stepStatus[step.status]}</small>
        </li>)}</ol>
        : <p className="agent-run-waiting">{copy.waiting}</p>}
      {artifacts.length > 0 && <p className="agent-run-artifacts">{copy.generated} · {artifacts.map((artifact) => copy.artifacts[artifact] ?? copy.artifactFallback).join(' · ')}</p>}
    </div>}
    {(task.status === 'needs_clarification' || task.status === 'needs_review') && (
      <p className="agent-run-attention">
        {task.status === 'needs_clarification'
          ? copy.needsAnswer
          : copy.needsReview}
      </p>
    )}
    {task.status === 'cancelled' && <p className="agent-run-attention">{copy.cancelled}</p>}
    {task.status === 'failed' && <p className="agent-run-attention error">{copy.failed}</p>}
  </section>
}
