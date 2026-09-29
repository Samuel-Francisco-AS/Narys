import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { startConversationTask } from './conversationTaskClient'
import { createSession, getSession } from '../conversation/conversationClient'
import { cancelTask, lunaCoreAvailable } from './taskClient'
import type { CognitiveResult, TaskEvent, TaskId, TaskState } from './types'
import type { AnimationIntent } from '../avatar/runtime/types'
import { taskEventToAnimationIntent } from './taskAnimation'

type Status = { configured: boolean; enabled: boolean; credentialStoreAvailable: boolean }
type Message = { id: number; role: string; content: string }

export default function GeminiPanel({ onAnimationIntent }: { onAnimationIntent: (intent: AnimationIntent) => void }) {
  const [status, setStatus] = useState<Status | null>(null)
  const [message, setMessage] = useState('')
  const [history, setHistory] = useState<Message[]>([])
  const [stream, setStream] = useState('')
  const [result, setResult] = useState<CognitiveResult | null>(null)
  const [state, setState] = useState<TaskState | null>(null)
  const [events, setEvents] = useState<string[]>([])
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const active = useRef<TaskId | null>(null)
  const busyRef = useRef(false)
  const generation = useRef(0)
  const sessionId = useRef<number | null>(null)

  async function refresh() {
    if (!lunaCoreAvailable) return
    try {
      setStatus(await invoke<Status>('gemini_status'))
      if (sessionId.current !== null) setHistory((await getSession(sessionId.current)).messages)
    } catch { setError('Não foi possível consultar Gemini ou o cofre de credenciais.') }
  }
  useEffect(() => {
    void refresh()
    return () => {
      generation.current += 1
      if (active.current !== null) void cancelTask(active.current).catch(() => {})
    }
  }, [])
  async function send() {
    if (!status?.configured || busyRef.current || !message.trim() || new TextEncoder().encode(message).length > 4096) return
    busyRef.current = true
    const run = ++generation.current
    let terminalSeen = false
    setBusy(true); setError(''); setStream(''); setResult(null); setEvents([]); setState('pending')
    try {
      if (sessionId.current === null) sessionId.current = await createSession()
      const id = await startConversationTask(sessionId.current, message, (event: TaskEvent) => {
        if (run !== generation.current) return
        if (event.state === 'pending' || event.state === 'running') active.current = event.taskId
        setState(event.state)
        const intent = taskEventToAnimationIntent(event)
        if (intent) onAnimationIntent(intent)
        if (event.type === 'provider_selected') setEvents((old) => [...old, `ProviderSelected: ${event.provider_id}`])
        if (event.type === 'context_built') setEvents((old) => [...old, 'ContextBuilt: memórias privadas enviadas 0'])
        if (event.type === 'provider_chunk') setStream((old) => old + event.chunk)
        if (event.type === 'task_result_ready') setResult(event.result)
        if (event.type === 'task_failed') setError(event.detail)
        if (event.type === 'task_completed' || event.type === 'task_cancelled' || event.type === 'task_failed') {
          terminalSeen = true; active.current = null; busyRef.current = false; setBusy(false)
          setEvents((old) => [...old, event.type])
          if (event.type === 'task_completed') { setMessage(''); void refresh() }
        }
      })
      if (run !== generation.current) { await cancelTask(id); return }
      if (!terminalSeen) active.current = id
    } catch {
      if (run === generation.current) { busyRef.current = false; setBusy(false); setError('Não foi possível iniciar a tarefa Gemini.') }
    }
  }
  return <section className="gemini-panel" aria-label="Chat Gemini LR-6">
    <h3>CHAT · LR-6</h3>
    {!lunaCoreAvailable ? <p>Gemini requer a janela Tauri. Nenhuma chave ou mensagem é enviada no navegador.</p> : <>
      <p>Gemini: {status?.configured ? 'configurado' : 'não configurado'} · Cofre: {status?.credentialStoreAvailable ? 'disponível' : 'indisponível'}</p>
      <p>Credencial e policy: gerenciar em Configurações → IA e modelos.</p>
      <p>Esta mensagem será enviada ao Gemini. No Free Tier, o conteúdo pode ser usado pelo Google para melhorar seus produtos. Memórias privadas da Luna não são enviadas nesta fase.</p>
      <textarea value={message} onChange={(e) => setMessage(e.target.value)} maxLength={4096} aria-label="Mensagem ao Gemini" />
      <div className="luna-core-controls">
        <button type="button" disabled={!status?.configured || busy || !message.trim()} onClick={() => void send()}>Enviar ao Gemini</button>
        <button type="button" disabled={!busy || active.current === null} onClick={() => { if (active.current !== null) void cancelTask(active.current) }}>Cancelar</button>
      </div>
      <p role="status">Estado: {state ?? 'pronto'} · Memórias privadas enviadas: 0</p>
      {error && <p role="alert">{error}</p>}
      {stream && <p className="gemini-response">{state === 'completed' ? 'Resposta: ' : 'Prévia não salva como resposta final: '}{stream}</p>}
      {result && <p>Provider: {result.providerId} · Tokens: entrada {result.usage.inputTokens}, saída {result.usage.outputTokens}, total {result.usage.totalTokens ?? '—'}, thinking {result.usage.thoughtTokens ?? '—'}</p>}
      <ul>{events.map((event, index) => <li key={index}>{event}</li>)}</ul>
      <h4>Conversa local</h4>
      <ol>{history.map((item) => <li key={item.id}>{item.role}: {item.content}</li>)}</ol>
    </>}
  </section>
}
