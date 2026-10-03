import { useEffect, useRef, useState } from 'react'
import type { AnimationIntent } from '../avatar/runtime/types'
import { taskEventToAnimationIntent } from './taskAnimation'
import { cancelTask, lunaCoreAvailable, startMockTask } from './taskClient'
import type { TaskEvent, TaskId, TaskState, TaskStep } from './types'

type Props = {
  onAnimationIntent: (intent: AnimationIntent) => void
}

const stepLabels: Record<TaskStep, string> = {
  prepare: 'Preparação',
  verify: 'Verificação',
}

const stateLabels: Record<TaskState, string> = {
  pending: 'pendente',
  running: 'executando',
  completed: 'concluída',
  cancelled: 'cancelada',
  failed: 'falhou',
}

function eventLabel(event: TaskEvent): string {
  switch (event.type) {
    case 'task_started': return 'Tarefa iniciada'
    case 'step_started': return `${stepLabels[event.step]} iniciada`
    case 'step_completed': return `${stepLabels[event.step]} concluída`
    case 'task_completed': return 'Tarefa concluída'
    case 'task_cancelled': return 'Tarefa cancelada'
    case 'task_failed': return `Tarefa falhou: ${event.detail}`
    case 'context_built': return `Contexto: ${event.memory_count} memórias`
    case 'provider_selected': return `${event.provider_id}: tentativa ${event.attempt}`
    case 'provider_chunk': return `Chunk: ${event.chunk}`
    case 'provider_retry': return `${event.provider_id}: retry (${event.reason_code})`
    case 'provider_fallback': return `${event.from_provider_id} → ${event.to_provider_id}: fallback (${event.reason_code})`
    case 'task_result_ready': return `Resultado: ${event.result.providerId}`
    case 'task_planned': return `Task graph: ${event.step_count} subtarefas planejadas`
    case 'subtask_waiting': return `${event.subtask_id}: aguardando ${event.depends_on.join(', ') || 'dependências'}`
    case 'subtask_started': return `${event.subtask_id}: ${event.provider_id} iniciou`
    case 'subtask_completed': return `${event.subtask_id}: ${event.provider_id} concluiu`
    case 'subtask_retry': return `${event.subtask_id}: retry em ${event.provider_id} (${event.reason_code})`
    case 'subtask_output_observed': return `${event.subtask_id}: saída observada de ${event.provider_id}`
    case 'subtask_failed': return `${event.subtask_id}: falhou (${event.error_code})`
    case 'task_graph_result_ready': return `Task graph: ${event.result.subtasks.length} resultados consolidados`
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

export default function LunaCorePanel({ onAnimationIntent }: Props) {
  const [taskId, setTaskId] = useState<TaskId | null>(null)
  const [taskState, setTaskState] = useState<TaskState | null>(null)
  const [events, setEvents] = useState<TaskEvent[]>([])
  const [starting, setStarting] = useState(false)
  const [cancelRequested, setCancelRequested] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const activeTaskRef = useRef<TaskId | null>(null)
  const busyRef = useRef(false)
  const mountedRef = useRef(true)
  const generationRef = useRef(0)

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
      generationRef.current += 1
      if (activeTaskRef.current !== null) void cancelTask(activeTaskRef.current).catch(console.error)
    }
  }, [])

  async function start() {
    if (!lunaCoreAvailable || busyRef.current) return
    busyRef.current = true
    const generation = ++generationRef.current
    setTaskId(null)
    setTaskState(null)
    setEvents([])
    setError(null)
    setCancelRequested(false)
    setStarting(true)

    try {
      const id = await startMockTask((event) => {
        if (!mountedRef.current || generation !== generationRef.current) return
        setTaskId(event.taskId)
        setTaskState(event.state)
        setEvents((current) => [...current, event])
        const intent = taskEventToAnimationIntent(event)
        if (intent) onAnimationIntent(intent)
        if (event.state === 'completed' || event.state === 'cancelled' || event.state === 'failed') {
          activeTaskRef.current = null
          busyRef.current = false
          setCancelRequested(false)
          setError(null)
          window.dispatchEvent(new Event('lr4-task-terminal'))
        } else {
          activeTaskRef.current = event.taskId
        }
      })
      if (!mountedRef.current || generation !== generationRef.current) {
        await cancelTask(id)
        return
      }
      setTaskId(id)
      setTaskState((current) => current ?? 'pending')
      if (busyRef.current) activeTaskRef.current = id
    } catch (cause) {
      if (mountedRef.current && generation === generationRef.current) {
        setError(`Não foi possível iniciar: ${errorMessage(cause)}`)
        busyRef.current = false
      }
    } finally {
      if (mountedRef.current && generation === generationRef.current) setStarting(false)
    }
  }

  async function cancel() {
    const id = activeTaskRef.current
    if (id === null || cancelRequested) return
    setCancelRequested(true)
    setError(null)
    try {
      const accepted = await cancelTask(id)
      if (mountedRef.current && !accepted) setError('A tarefa já não está ativa; aguardando o evento final.')
    } catch (cause) {
      if (mountedRef.current) {
        setError(`Não foi possível cancelar: ${errorMessage(cause)}`)
        setCancelRequested(false)
      }
    }
  }

  const active = taskState === 'pending' || taskState === 'running'

  return (
    <aside className="conversation-panel" aria-label="Diagnóstico do Luna Core">
      <div className="panel-heading">
        <div>
          <span className="section-label">LUNA CORE · LR-2</span>
          <h2>Tarefa de diagnóstico</h2>
        </div>
      </div>
      <div className="luna-core-body">
        <p className="luna-core-description">Fluxo experimental de tarefas e eventos do núcleo Rust.</p>
        <div className="luna-core-controls">
          <button type="button" className="primary-button" disabled={!lunaCoreAvailable || busyRef.current} onClick={start}>
            Executar tarefa mock
          </button>
          <button type="button" className="secondary-button" disabled={!active || taskId === null || cancelRequested} onClick={cancel}>
            Cancelar
          </button>
        </div>
        {!lunaCoreAvailable && <p className="luna-core-note">Luna Core requer a janela Tauri. O avatar continua disponível no navegador.</p>}
        <p className="luna-core-summary" role="status">
          TaskId: {taskId ?? '—'} · Estado: {starting && !taskState ? 'solicitando início' : taskState ? stateLabels[taskState] : 'sem tarefa'}
        </p>
        {cancelRequested && active && <p className="luna-core-note">Cancelamento solicitado; aguardando o Luna Core.</p>}
        {error && <p className="luna-core-error" role="alert">{error}</p>}
        <h3 className="luna-core-events-title">Eventos</h3>
        <ol className="luna-core-events" aria-live="polite">
          {events.map((event) => <li key={`${event.taskId}-${event.sequence}`}>{eventLabel(event)}</li>)}
        </ol>
      </div>
    </aside>
  )
}
