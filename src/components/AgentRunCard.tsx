// 展示单个 Agent task 的状态和运行步骤；步骤文案面向用户，不暴露内部工具名。
import { useEffect, useMemo, useState } from 'react'
import { listAgentRunSteps } from '../lib/local-store'
import type { StoredAgentRunStep, StoredAgentTask } from '../lib/local-store'

type AgentRunCardProps = {
  task: StoredAgentTask
  onOpenStoryboard: () => void
}

const ACTIVE_TASK_STATUSES = new Set<StoredAgentTask['status']>(['queued', 'running'])

type StageId = 'analyze' | 'select' | 'edit' | 'preview' | 'deliver' | 'finish'

const STAGE_LABELS: Record<StageId, string> = {
  analyze: '分析素材',
  select: '选择镜头',
  edit: '生成剪辑',
  preview: '生成预览',
  deliver: '准备剪映草稿',
  finish: '整理结果',
}

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

const TOOL_LABELS: Record<string, string> = {
  agent_loop: '执行剪辑',
  read_logs: '查看运行情况',
  list_assets: '检查可用素材',
  search_assets: '查找合适素材',
  search_asset_segments: '查找可用片段',
  request_asset_analysis: '分析素材',
  retry_failed_asset_analysis: '重试素材分析',
  get_asset_health_summary: '检查素材状态',
  get_storyboard: '查看已选镜头',
  generate_storyboard: '选择镜头',
  get_timeline: '检查当前剪辑',
  get_text_capabilities: '检查字幕能力',
  create_timeline_draft: '生成剪辑',
  replace_clips: '替换镜头',
  insert_clips: '补足镜头',
  change_clip_duration: '调整镜头时长',
  reorder_clips: '调整镜头顺序',
  replace_text_tracks: '设计字幕',
  replace_music_tracks: '添加音乐',
  search_music: '搜索音乐',
  download_music: '下载音乐',
  use_online_music: '添加在线音乐',
  list_voices: '选择配音音色',
  synthesize_voiceover: '生成配音',
  render_preview: '生成预览',
  create_jianying_draft: '准备剪映草稿',
  finish: '整理结果',
  done: '整理结果',
  no_action: '确认无需操作',
}

const ARTIFACT_LABELS: Record<string, string> = {
  storyboard_version: '镜头方案',
  timeline_version: '剪辑结果',
  preview: '预览',
  jianying_draft: '剪映草稿',
  asset_analysis: '素材分析',
}

function toolStage(toolName: string): StageId {
  return TOOL_STAGES[toolName] ?? 'edit'
}

function toolLabel(toolName: string) {
  return TOOL_LABELS[toolName] ?? '继续处理'
}

function taskStatusCopy(status: StoredAgentTask['status']) {
  if (status === 'completed') return '已完成'
  if (status === 'partially_completed') return '部分完成'
  if (status === 'needs_clarification') return '需要你的回答'
  if (status === 'needs_review') return '需要检查'
  if (status === 'cancelled') return '已停止'
  if (status === 'failed') return '未能完成'
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
    ? STAGE_LABELS[toolStage(currentStep.toolName)]
    : ACTIVE_TASK_STATUSES.has(task.status)
      ? '正在准备下一步'
      : task.status === 'completed'
        ? '完成'
        : taskStatusCopy(task.status)
  const hasResult = artifacts.includes('storyboard_version')
    || artifacts.includes('timeline_version')
    || artifacts.includes('preview')

  return <section className={`agent-run-card ${task.status}`} aria-live={ACTIVE_TASK_STATUSES.has(task.status) ? 'polite' : 'off'}>
    <button className="agent-run-summary" type="button" onClick={() => setExpanded((value) => !value)} aria-expanded={expanded}>
      <span className={`agent-run-state ${task.status}`} aria-hidden="true" />
      <span className="agent-run-copy">
        <strong>{currentCopy}</strong>
        <small>{taskStatusCopy(task.status)} · 已完成 {completedCount} 步 · {elapsedCopy(task)}</small>
      </span>
      <span className="agent-run-toggle">{expanded ? '收起' : '查看步骤'}</span>
    </button>
    {expanded && <div className="agent-run-details">
      {sortedSteps.length > 0
        ? <ol className="agent-step-list">{sortedSteps.map((step) => <li className={step.status} key={step.id}>
          <span className="agent-step-icon" aria-hidden="true" />
          <span><strong>{toolLabel(step.toolName)}</strong><small>{stepStatusCopy(step.status)}</small></span>
        </li>)}</ol>
        : <p className="agent-run-waiting">任务已开始，正在等待下一步。</p>}
      {artifacts.length > 0 && (
        <div className="agent-run-artifacts">
          <strong>已生成</strong>
          <ul>
            {artifacts.map((artifact) => (
              <li key={artifact}>
                <span>✓</span>
                {ARTIFACT_LABELS[artifact] ?? '本地结果'}
              </li>
            ))}
          </ul>
          {hasResult && <button type="button" onClick={onOpenStoryboard}>查看成果</button>}
        </div>
      )}
      {(task.status === 'needs_clarification' || task.status === 'needs_review') && (
        <p className="agent-run-attention">
          {task.status === 'needs_clarification'
            ? '请在对话中回答问题后继续。'
            : '上次处理意外中断，已有结果不会自动重做，请检查后重新运行。'}
        </p>
      )}
      {task.status === 'cancelled' && <p className="agent-run-attention">已停止本轮处理；已有结果不会被自动覆盖。</p>}
      {task.status === 'failed' && <p className="agent-run-attention error">这次没有完成；已有结果不会被自动覆盖。</p>}
    </div>}
  </section>
}
