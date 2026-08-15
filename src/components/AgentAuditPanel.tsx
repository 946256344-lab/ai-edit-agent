// 只读展示当前 conversation 的 Agent task、操作日志和 timeline 版本审计信息。
import type { StoredAgentTask, StoredOperationLog, TimelineVersion } from '../lib/local-store'

type AgentAuditPanelProps = {
  tasks: StoredAgentTask[]
  logs: StoredOperationLog[]
  timelineVersions: TimelineVersion[]
}

function taskStatus(status: StoredAgentTask['status']) {
  if (status === 'needs_review') return '待审阅'
  if (status === 'needs_clarification') return '待澄清'
  if (status === 'partially_completed') return '部分完成'
  if (status === 'completed') return '已完成'
  if (status === 'failed') return '失败'
  if (status === 'running') return '执行中'
  return '等待中'
}

export function AgentAuditPanel({ tasks, logs, timelineVersions }: AgentAuditPanelProps) {
  if (!tasks.length && !logs.length && !timelineVersions.length) return null
  return (
    <section className="plan-card audit-card">
      <div className="plan-heading">
        <span>Agent 审计</span>
        <small>仅显示当前剪辑会话的本地记录</small>
      </div>
      {tasks.length > 0 && (
        <>
          <strong>工具调用</strong>
          <ul className="audit-list">
            {tasks.slice(0, 6).map((task) => (
              <li key={task.id}>
                <span className={`audit-status ${task.status}`}>{taskStatus(task.status)}</span>
                <b>{task.toolName}</b>
                <small>
                  {new Date(task.updatedAt).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })}
                  {task.error ? ` · ${task.error}` : ''}
                </small>
              </li>
            ))}
          </ul>
        </>
      )}
      {logs.length > 0 && (
        <>
          <strong>副作用记录</strong>
          <ul className="audit-list">
            {logs.slice(0, 4).map((log) => (
              <li key={log.id}>
                <span className="audit-status completed">已记录</span>
                <b>{log.operationType}</b>
                <small>{new Date(log.createdAt).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })}</small>
              </li>
            ))}
          </ul>
        </>
      )}
      {timelineVersions.length > 0 && (
        <>
          <strong>时间线版本</strong>
          <ul className="audit-list">
            {timelineVersions.slice(0, 4).map((timeline) => (
              <li key={timeline.id}>
                <span className="audit-status completed">v{timeline.versionNumber}</span>
                <b>{timeline.clips.length} 个镜头</b>
                <small>{new Date(timeline.createdAt).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })}</small>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  )
}
