import { useEffect, useRef, useState } from 'react'
import { cancelTask, lunaCoreAvailable } from '../luna/taskClient'
import { attachConversationEvents, getCurrentInteraction, startConversationTask } from '../luna/conversationTaskClient'
import { closeSession, conversationRoutingStatus, createSession, getSession, resumeConversationSession } from './conversationClient'
import type { TaskEvent } from '../luna/types'
import type { ConversationSession, ConversationState } from './types'

const taskFailure = (detail: string, cooldownMs = 0) => {
  const wait = cooldownMs > 0 ? ` Tente novamente em cerca de ${Math.max(1, Math.ceil(cooldownMs / 1000))} s.` : ''
  const known: Record<string, string> = {
    model_or_request_rejected: 'Modelo ou parâmetros rejeitados pelo provider selecionado. Revise IA e modelos.',
    rate_limited: 'Os providers elegíveis limitaram as chamadas.',
    provider_unavailable: 'Nenhum provider elegível está disponível agora.',
    unavailable: 'O provider selecionado está temporariamente indisponível.',
    timeout: 'A chamada ao provider excedeu o tempo configurado.',
    provider_auth_failed: 'A credencial do provider selecionado foi rejeitada ou está ausente. Revise IA e modelos.',
  }
  return `${known[detail] ?? `Falha na resposta (${detail}).`}${wait} Mensagem não enviada.`
}

const initial: ConversationState = { sessionId: null, messages: [], draft: '', preview: '', assistantStreaming: false, activeTaskId: null, providerCooldownUntil: null, providerRoute: null, error: null }

export function useConversationController() {
  const [state, setState] = useState<ConversationState>(initial)
  const current = useRef(initial)
  const busy = useRef(false)
  const hydrated = useRef(!lunaCoreAvailable)
  const generation = useRef(0)
  const change = (patch: Partial<ConversationState>) => {
    current.current = { ...current.current, ...patch }
    setState(current.current)
  }
  useEffect(() => {
    if (!lunaCoreAvailable) return
    const run = ++generation.current
    busy.current = true
    const recover = async () => {
      const interaction = await getCurrentInteraction()
      if (run !== generation.current || interaction.sessionId === null) return
      const sessionId = interaction.sessionId
      const session = await getSession(sessionId)
      if (run !== generation.current) return
      change({ sessionId, messages: session.messages })
      if (!interaction.task) return
      let cursor = 0
      let attached = false
      const pending: TaskEvent[] = []
      const observe = (event: TaskEvent) => {
        if (run !== generation.current || event.taskId !== interaction.task?.taskId || event.sequence <= cursor) return
        cursor = event.sequence
        if (event.type === 'provider_chunk') change({ preview: current.current.preview + event.chunk })
        if (event.type === 'provider_selected') change({ providerRoute: `Provider ativo: ${event.provider_id} · ${event.routing_reason}` })
        if (event.type === 'task_completed' || event.type === 'task_failed' || event.type === 'task_cancelled') {
          busy.current = false
          change({ activeTaskId: null, assistantStreaming: false, preview: '', error: event.type === 'task_failed' ? taskFailure(event.detail) : event.type === 'task_cancelled' ? 'Resposta cancelada.' : null })
          void refresh(sessionId, run).catch(() => { if (run === generation.current) change({ error: 'Não foi possível atualizar a conversa local.' }) })
        }
      }
      const snapshot = await attachConversationEvents(interaction.task.taskId, sessionId, 0, event => {
        if (!attached) pending.push(event); else observe(event)
      })
      if (run !== generation.current) return
      // Historical streaming may be truncated. Show only factual running state,
      // then future chunks; the complete answer is loaded from SQLite on terminal.
      cursor = snapshot.sequence
      const running = snapshot.state === 'pending' || snapshot.state === 'running'
      busy.current = running
      change({ activeTaskId: running ? snapshot.taskId : null, assistantStreaming: running, preview: '',
        providerRoute: running ? `Resposta em andamento… · tarefa #${snapshot.taskId}` : null,
        error: snapshot.terminal?.type === 'task_failed' ? taskFailure(snapshot.terminal.detail) : snapshot.state === 'cancelled' ? 'Resposta cancelada.' : null })
      if (!running) await refresh(sessionId, run)
      attached = true
      pending.forEach(observe)
    }
    void recover().then(() => { if (run === generation.current) hydrated.current = true }).catch(() => { if (run === generation.current) change({ error: 'Não foi possível recuperar a Interaction. Reabra a interface antes de enviar.' }) })
      .finally(() => { if (run === generation.current && !current.current.assistantStreaming) busy.current = false })
    // Unmount releases only the visual subscriber; backend owns HeadlessSafe execution.
    return () => { generation.current += 1 }
  }, [])
  const refresh = async (id: number, run: number) => {
    const session = await getSession(id)
    if (run === generation.current) change({ messages: session.messages, preview: '' })
  }
  const send = async () => {
    const message = current.current.draft.trim()
    if (!hydrated.current || busy.current || !message) return false
    if (!lunaCoreAvailable) { change({ error: 'Conversa disponível somente no aplicativo desktop.' }); return false }
    if (new TextEncoder().encode(message).length > 4096) { change({ error: 'Mensagem longa demais (máximo de 4096 bytes).' }); return false }
    busy.current = true
    const run = ++generation.current
    let id = current.current.sessionId
    change({ draft: '', error: null, preview: '', providerRoute: null, assistantStreaming: true,
      messages: [...current.current.messages, { id: -run, sessionId: id ?? 0, role: 'user', content: message, createdAt: '' }] })
    const rollback = (error: string, cooldownMs = 0) => change({ draft: message, assistantStreaming: false, preview: '', activeTaskId: null,
      providerCooldownUntil: cooldownMs > 0 ? Date.now() + cooldownMs : null,
      messages: current.current.messages.filter((item) => item.id !== -run), error })
    try {
      const routing = await conversationRoutingStatus()
      const providerLabel = (providerId: string) => routing.targets.find(provider => provider.providerId === providerId)?.displayName ?? providerId
      for (const target of routing.targets) {
        if (!target.configured) throw new Error(`Configure a chave ${providerLabel(target.providerId)} em Configurações → IA e modelos antes de conversar.`)
      }
      const ready = routing.targets.filter(target => target.cooldownMs === 0)
      if (ready.length === 0) {
        const wait = Math.min(...routing.targets.map(target => target.cooldownMs))
        rollback(taskFailure('provider_unavailable', wait), wait); busy.current = false; return false
      }
      if (routing.targets[0]?.cooldownMs > 0) change({ providerRoute: `Targets elegíveis: ${ready.map(target => providerLabel(target.providerId)).join(' → ')}` })
      if (id === null) id = await createSession()
      if (run !== generation.current) return false
      const sessionId = id
      change({ sessionId })
      let terminal = false
      let selectionDetail = ''
      const taskId = await startConversationTask(sessionId, message, (event) => {
        if (run !== generation.current) return
        if (event.state === 'pending' || event.state === 'running') change({ activeTaskId: event.taskId })
        if (event.type === 'provider_selected') {
          selectionDetail = `${event.routing_reason} · score ${event.score ?? '—'} · tentativa ${event.attempt}`
          change({ providerRoute: `Provider ativo: ${providerLabel(event.provider_id)} · ${selectionDetail}` })
        }
        if (event.type === 'provider_fallback') change({ providerRoute: `Fallback real: ${providerLabel(event.from_provider_id)} → ${providerLabel(event.to_provider_id)} · ${event.reason_code}` })
        if (event.type === 'provider_chunk') change({ preview: current.current.preview + event.chunk })
        if (event.type === 'task_result_ready') {
          const providers = event.result.usage.providersUsed.map(providerLabel).join(' → ')
          change({ providerRoute: `Resposta concluída por ${providerLabel(event.result.providerId)} · ${selectionDetail}${providers ? ` · rota ${providers}` : ''}` })
        }
        if (event.type === 'task_completed' || event.type === 'task_cancelled' || event.type === 'task_failed') {
          terminal = true
          change({ activeTaskId: null })
          if (event.type === 'task_completed') {
            void refresh(sessionId, run)
              .then(() => { if (run === generation.current) { busy.current = false; change({ assistantStreaming: false, providerCooldownUntil: null, error: null }) } })
              .catch(() => { if (run === generation.current) { busy.current = false; change({ assistantStreaming: false, preview: '', error: 'Resposta concluída; não foi possível atualizar a conversa local.' }) } })
          } else if (event.type === 'task_failed') {
            busy.current = false
            void conversationRoutingStatus().then(status => {
              if (run !== generation.current) return
              const allCooling = status.targets.length > 0 && status.targets.every(target => target.cooldownMs > 0)
              const wait = allCooling ? Math.min(...status.targets.map(target => target.cooldownMs)) : 0
              rollback(taskFailure(event.detail, wait), wait)
            }).catch(() => {
              if (run === generation.current) rollback(taskFailure(event.detail))
            })
          } else {
            busy.current = false
            rollback('Resposta cancelada. Mensagem não enviada.')
          }
        }
      })
      if (run !== generation.current) return false
      if (!terminal) change({ activeTaskId: taskId })
      return true
    } catch (error) {
      if (run === generation.current) rollback(error instanceof Error ? error.message : 'Não foi possível iniciar a conversa.')
      return false
    } finally {
      if (!current.current.assistantStreaming) busy.current = false
    }
  }
  const cancel = () => { if (current.current.activeTaskId !== null) void cancelTask(current.current.activeTaskId) }
  const newConversation = async () => {
    if (busy.current) return false
    busy.current = true
    const id = current.current.sessionId
    if (id !== null) {
      try { await closeSession(id) }
      catch { change({ error: 'Não foi possível encerrar a sessão atual.' }); busy.current = false; return false }
    }
    generation.current += 1
    change(initial)
    busy.current = false
    return true
  }
  const adoptSession = (session: ConversationSession) => {
    generation.current += 1
    change({ sessionId: session.id, messages: session.messages, draft: '', preview: '', assistantStreaming: false, activeTaskId: null, providerCooldownUntil: null, providerRoute: null, error: null })
  }
  const resumeConversation = async (targetId: number) => {
    if (busy.current || current.current.assistantStreaming) return false
    busy.current = true
    try {
      const session = await resumeConversationSession(targetId, current.current.sessionId)
      adoptSession(session)
      return true
    } finally { busy.current = false }
  }
  return { state, setDraft: (draft: string) => change({ draft }), send, cancel, newConversation, resumeConversation }
}
