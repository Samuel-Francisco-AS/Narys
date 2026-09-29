import { useEffect, useRef, useState } from 'react'
import { Channel, invoke, isTauri } from '@tauri-apps/api/core'
import type { AnimationIntent } from '../avatar/runtime/types'
import { taskEventToAnimationIntent } from './taskAnimation'
import { cancelTask } from './taskClient'
import type { CognitiveResult, TaskEvent, TaskId, TaskState } from './types'

type Scenario = 'normal' | 'streaming' | 'rate_limit_fallback' | 'timeout_retry' | 'budget_exhausted' | 'cancel'
type Status = { id: string; enabled: boolean; priority: number; cooldownMs: number; capabilities: { textGeneration: boolean; streaming: boolean } }
const scenarios: { value: Scenario; label: string }[] = [
  { value: 'normal', label: 'Normal' }, { value: 'streaming', label: 'Streaming' },
  { value: 'rate_limit_fallback', label: 'Rate limit + fallback' }, { value: 'timeout_retry', label: 'Timeout + retry' },
  { value: 'budget_exhausted', label: 'Budget exhausted' }, { value: 'cancel', label: 'Cancelamento' },
]
function label(event: TaskEvent): string {
  switch (event.type) {
    case 'context_built': return `Contexto montado: ${event.memory_count} memórias, ${event.recent_message_count} mensagens recentes`
    case 'provider_selected': return `${event.provider_id} · tentativa ${event.attempt}`
    case 'provider_chunk': return `Chunk de ${event.provider_id}: ${event.chunk}`
    case 'provider_retry': return `Retry ${event.provider_id}: ${event.reason_code}`
    case 'provider_fallback': return `Fallback ${event.from_provider_id} → ${event.to_provider_id}: ${event.reason_code}`
    case 'task_result_ready': return `Resultado pronto: ${event.result.providerId}`
    case 'task_started': return 'Tarefa iniciada'
    case 'task_completed': return 'Tarefa concluída'
    case 'task_cancelled': return 'Tarefa cancelada'
    case 'task_failed': return `Falha: ${event.detail}`
    default: return event.type
  }
}
export default function CognitionPanel({ onAnimationIntent }: { onAnimationIntent: (intent: AnimationIntent) => void }) {
  const available = isTauri() && import.meta.env.DEV
  const [scenario, setScenario] = useState<Scenario>('normal')
  const [taskId, setTaskId] = useState<TaskId | null>(null)
  const [state, setState] = useState<TaskState | null>(null)
  const [events, setEvents] = useState<TaskEvent[]>([])
  const [result, setResult] = useState<CognitiveResult | null>(null)
  const [statuses, setStatuses] = useState<Status[]>([])
  const [error, setError] = useState<string | null>(null)
  const activeRef = useRef<TaskId | null>(null)
  const busyRef = useRef(false)
  const mountedRef = useRef(true)
  useEffect(() => {
    mountedRef.current = true
    if (available) void invoke<Status[]>('cognition_provider_status').then(setStatuses).catch(() => setError('Falha ao consultar providers mock.'))
    return () => { mountedRef.current = false; if (activeRef.current !== null) void cancelTask(activeRef.current) }
  }, [available])
  async function start() {
    if (!available || busyRef.current) return
    busyRef.current = true
    setTaskId(null); setState('pending'); setEvents([]); setResult(null); setError(null)
    try {
      const channel = new Channel<TaskEvent>((event) => {
        if (!mountedRef.current) return
        setTaskId(event.taskId); setState(event.state); setEvents(current => [...current, event])
        if (event.type === 'task_result_ready') setResult(event.result)
        const intent = taskEventToAnimationIntent(event)
        if (intent) onAnimationIntent(intent)
        if (['completed', 'cancelled', 'failed'].includes(event.state)) {
          busyRef.current = false; activeRef.current = null
          window.dispatchEvent(new Event('lr4-task-terminal'))
          void invoke<Status[]>('cognition_provider_status').then(setStatuses).catch(() => {})
        } else activeRef.current = event.taskId
      })
      const id = await invoke<TaskId>('start_mock_cognition_task', { scenario, channel })
      if (!mountedRef.current) { await cancelTask(id); return }
      setTaskId(id)
      if (busyRef.current) activeRef.current = id
    } catch {
      if (mountedRef.current) { busyRef.current = false; setState(null); setError('Não foi possível iniciar diagnóstico cognitivo.') }
    }
  }
  async function cancel() {
    if (activeRef.current === null) return
    try { await cancelTask(activeRef.current) } catch { setError('Falha ao solicitar cancelamento.') }
  }
  const selected = [...events].reverse().find(event => event.type === 'provider_selected')
  const provider = selected?.type === 'provider_selected' ? selected.provider_id : '—'
  const attempts = events.filter(event => event.type === 'provider_selected').length
  const fallbacks = events.filter(event => event.type === 'provider_fallback').length
  const memoryCount = events.find(event => event.type === 'context_built')
  const stream = events.filter(event => event.type === 'provider_chunk').map(event => event.type === 'provider_chunk' ? event.chunk : '').join('')
  return <section className="security-panel" aria-label="Diagnóstico cognitivo LR-5">
    <strong>COGNITION · LR-5</strong>
    {!available ? <span>Requer Luna Core/Tauri em desenvolvimento. Nenhum provider roda no navegador.</span> : <>
      <label>Cenário <select value={scenario} disabled={busyRef.current} onChange={event => setScenario(event.target.value as Scenario)}>
        {scenarios.map(item => <option key={item.value} value={item.value}>{item.label}</option>)}
      </select></label>
      <div className="luna-core-controls">
        <button type="button" className="secondary-button" disabled={busyRef.current} onClick={() => void start()}>Executar diagnóstico</button>
        <button type="button" className="secondary-button" disabled={activeRef.current === null || state !== 'running'} onClick={() => void cancel()}>Cancelar</button>
      </div>
      <span>TaskId: {taskId ?? '—'} · Estado: {state ?? 'sem tarefa'}</span>
      <span>Provider: {result?.providerId ?? provider} · Tentativas: {result?.usage.providerCalls ?? attempts} · Fallbacks: {result?.usage.fallbacks ?? fallbacks}</span>
      <span>Tokens mock: entrada {result?.usage.inputTokens ?? '—'} · saída {result?.usage.outputTokens ?? '—'} · Memórias selecionadas: {memoryCount?.type === 'context_built' ? memoryCount.memory_count : '—'}</span>
      <span>Registry: {statuses.map(item => `${item.id} (${item.enabled ? 'on' : 'off'}, prioridade ${item.priority}, cooldown ${item.cooldownMs}ms)`).join(' · ') || '—'}</span>
      {stream && <span aria-live="polite">Stream: {stream}</span>}
      {result && <span>Resultado: {result.text}</span>}
      {error && <span className="luna-core-error" role="alert">{error}</span>}
      <ol className="luna-core-events" aria-live="polite">{events.map(event => <li key={`${event.taskId}-${event.sequence}`}>{label(event)}</li>)}</ol>
    </>}
  </section>
}
