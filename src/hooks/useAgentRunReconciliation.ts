import { useCallback, useEffect, useRef, useState } from 'react'
import type { Dispatch, RefObject, SetStateAction } from 'react'
import { listen } from '@tauri-apps/api/event'
import { listAgentTasks } from '../lib/local-store'
import type { AgentEditEvent, StoredAgentTask } from '../lib/local-store'

export type PendingAgentEdit = {
  taskId: string
  projectId: string
  sessionId: string
  conversationId: string
}

type AgentRunReconciliationOptions = {
  desktopRuntime: boolean
  projectId: string | null
  sessionId: string | null
  conversationId: string | null | undefined
  sessionState: 'ready' | 'working' | 'review' | undefined
  isSending: boolean
  tasks: StoredAgentTask[]
  activeProjectRef: RefObject<string | null>
  activeSessionRef: RefObject<string | null>
  setTasks: Dispatch<SetStateAction<StoredAgentTask[]>>
  setIsSending: Dispatch<SetStateAction<boolean>>
  setComposerNotice: Dispatch<SetStateAction<string | null>>
  applyCompletion: (pending: PendingAgentEdit, event?: AgentEditEvent) => Promise<void>
}

const ACTIVE_TASK_STATUSES = new Set<StoredAgentTask['status']>(['queued', 'running'])

function isActiveTask(task: StoredAgentTask) {
  return ACTIVE_TASK_STATUSES.has(task.status)
}

function isTerminalTask(task: StoredAgentTask) {
  return !isActiveTask(task)
}

/**
 * Reconciles an asynchronous Agent run with persisted task/message state.
 * Tauri events are only low-latency notifications; polling and the scoped
 * completion callback recover the authoritative SQLite result.
 */
export function useAgentRunReconciliation(options: AgentRunReconciliationOptions) {
  const { setComposerNotice, setIsSending } = options
  const [listenerReady, setListenerReady] = useState(!options.desktopRuntime)
  const pendingEditRef = useRef<PendingAgentEdit | null>(null)
  const earlyEventsRef = useRef<Map<string, AgentEditEvent>>(new Map())
  const observedActiveTaskIdsRef = useRef<Set<string>>(new Set())
  const reconcilingTaskIdsRef = useRef<Set<string>>(new Set())
  const reconciledTaskIdsRef = useRef<Set<string>>(new Set())
  const unlistenRef = useRef<(() => void) | null>(null)
  const listenerPromiseRef = useRef<Promise<boolean> | null>(null)
  const applyCompletionRef = useRef(options.applyCompletion)

  useEffect(() => {
    applyCompletionRef.current = options.applyCompletion
  }, [options.applyCompletion])

  const reconcileCompletion = useCallback((pending: PendingAgentEdit, event?: AgentEditEvent) => {
    const taskId = pending.taskId
    if (reconciledTaskIdsRef.current.has(taskId) || reconcilingTaskIdsRef.current.has(taskId)) return
    const controlsComposer = pendingEditRef.current?.taskId === taskId
    reconcilingTaskIdsRef.current.add(taskId)
    if (controlsComposer) pendingEditRef.current = null
    earlyEventsRef.current.delete(taskId)
    void applyCompletionRef.current(pending, event)
      .then(() => {
        const reconciled = reconciledTaskIdsRef.current
        reconciled.add(taskId)
        if (reconciled.size > 100) {
          const oldestTaskId = reconciled.values().next().value
          if (oldestTaskId) reconciled.delete(oldestTaskId)
        }
        observedActiveTaskIdsRef.current.delete(taskId)
      })
      .catch(() => {
        setComposerNotice('Agent 已完成，但界面状态同步失败；请切换任务或重启应用后查看持久化结果。')
      })
      .finally(() => {
        reconcilingTaskIdsRef.current.delete(taskId)
        if (controlsComposer) setIsSending(false)
      })
  }, [setComposerNotice, setIsSending])

  function receiveCompletion(event: AgentEditEvent) {
    if (reconciledTaskIdsRef.current.has(event.agentTaskId)) return
    const pending = pendingEditRef.current
    if (!pending || event.agentTaskId !== pending.taskId) {
      const earlyEvents = earlyEventsRef.current
      earlyEvents.set(event.agentTaskId, event)
      if (earlyEvents.size > 20) {
        const oldestTaskId = earlyEvents.keys().next().value
        if (oldestTaskId) earlyEvents.delete(oldestTaskId)
      }
      return
    }
    reconcileCompletion(pending, event)
  }

  function ensureListener() {
    if (!options.desktopRuntime || unlistenRef.current) return Promise.resolve(true)
    if (listenerPromiseRef.current) return listenerPromiseRef.current
    const listenerPromise = listen<AgentEditEvent>('agent-edit-completed', (event) => receiveCompletion(event.payload))
      .then((unlisten) => {
        unlistenRef.current = unlisten
        setListenerReady(true)
        return true
      })
      .catch(() => {
        setListenerReady(false)
        return false
      })
      .finally(() => {
        listenerPromiseRef.current = null
      })
    listenerPromiseRef.current = listenerPromise
    return listenerPromise
  }

  function registerPendingEdit(pending: PendingAgentEdit) {
    pendingEditRef.current = pending
    observedActiveTaskIdsRef.current.add(pending.taskId)
    const earlyCompletion = earlyEventsRef.current.get(pending.taskId)
    if (earlyCompletion) receiveCompletion(earlyCompletion)
  }

  useEffect(() => {
    const { desktopRuntime, projectId, sessionId, conversationId, sessionState, isSending, tasks } = options
    if (!desktopRuntime || !projectId || !sessionId || !conversationId) return
    if (!isSending && sessionState !== 'working') return
    let cancelled = false
    for (const task of tasks) {
      if (isActiveTask(task)) observedActiveTaskIdsRef.current.add(task.id)
    }
    const refresh = () => void listAgentTasks(projectId, sessionId, conversationId)
      .then((nextTasks) => {
        if (cancelled || options.activeProjectRef.current !== projectId || options.activeSessionRef.current !== sessionId) return
        options.setTasks(nextTasks)
        for (const task of nextTasks) {
          if (isActiveTask(task)) observedActiveTaskIdsRef.current.add(task.id)
        }
        const pending = pendingEditRef.current
        if (pending && pending.projectId === projectId && pending.sessionId === sessionId) {
          const terminalPending = nextTasks.find((task) => task.id === pending.taskId && isTerminalTask(task))
          if (terminalPending) reconcileCompletion(pending, earlyEventsRef.current.get(pending.taskId))
          return
        }
        const observedTerminal = nextTasks.find((task) => (
          isTerminalTask(task)
          && observedActiveTaskIdsRef.current.has(task.id)
          && !reconciledTaskIdsRef.current.has(task.id)
        ))
        const snapshotHasActiveTask = nextTasks.some(isActiveTask)
        const persistedWorkingTerminal = !isSending && sessionState === 'working' && !snapshotHasActiveTask
          ? nextTasks.find((task) => isTerminalTask(task) && !reconciledTaskIdsRef.current.has(task.id))
          : undefined
        const terminalToReconcile = observedTerminal ?? persistedWorkingTerminal
        if (terminalToReconcile) {
          reconcileCompletion(
            { taskId: terminalToReconcile.id, projectId, sessionId, conversationId },
            earlyEventsRef.current.get(terminalToReconcile.id),
          )
        }
      })
      .catch(() => undefined)
    refresh()
    const intervalId = window.setInterval(refresh, 1200)
    return () => {
      cancelled = true
      window.clearInterval(intervalId)
    }
    // Composer ownership or persisted working state keeps polling alive until reconciliation.
    // Task status changes never control the interval lifetime.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [
    options.conversationId,
    options.desktopRuntime,
    options.isSending,
    options.projectId,
    options.sessionId,
    options.sessionState,
  ])

  useEffect(() => {
    if (!options.desktopRuntime || !options.isSending) return
    const pending = pendingEditRef.current
    if (!pending) return
    const pendingTask = options.tasks.find((task) => task.id === pending.taskId)
    if (pendingTask && isTerminalTask(pendingTask)) {
      reconcileCompletion(pending, earlyEventsRef.current.get(pending.taskId))
    }
  }, [options.desktopRuntime, options.isSending, options.tasks, reconcileCompletion])

  useEffect(() => {
    if (!options.desktopRuntime) return
    const restoreListener = () => void ensureListener()
    restoreListener()
    window.addEventListener('focus', restoreListener)
    const retryInterval = window.setInterval(restoreListener, 3000)
    return () => {
      window.removeEventListener('focus', restoreListener)
      window.clearInterval(retryInterval)
      unlistenRef.current?.()
      unlistenRef.current = null
      setListenerReady(false)
    }
    // The listener is process-scoped; event handlers read current scope from refs.
    // oxlint-disable-next-line react-hooks/exhaustive-deps
  }, [options.desktopRuntime])

  return { listenerReady, ensureListener, registerPendingEdit }
}
