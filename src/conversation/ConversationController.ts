import { useEffect, useRef, useState } from 'react'
import { cancelTask, lunaCoreAvailable } from '../luna/taskClient'
import { startConversationTask } from '../luna/conversationTaskClient'
import { closeSession, conversationRoutingStatus, createSession, getSession, resumeConversationSession } from './conversationClient'
import type { ConversationSession, ConversationState } from './types'

const providerLabel = (id: string) => id === 'gemini' ? 'Gemini' : id === 'groq' ? 'Groq' : id
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
  const generation = useRef(0)
  const change = (patch: Partial<ConversationState>) => {
    current.current = { ...current.current, ...patch }
    setState(current.current)
  }
  useEffect(() => () => { generation.current += 1; if (current.current.activeTaskId !== null) void cancelTask(current.current.activeTaskId) }, [])
  const refresh = async (id: number, run: number) => {
    const session = await getSession(id)
    if (run === generation.current) change({ messages: session.messages, preview: '' })
  }
  const send = async () => {
    const message = current.current.draft.trim()
    if (busy.current || !message) return false
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
      if (!routing.primary.configured) throw new Error(`Configure a chave ${providerLabel(routing.primary.providerId)} em Configurações → IA e modelos antes de conversar.`)
      const fallbackReady = routing.routingMode === 'preferred' && routing.fallback?.configured === true && routing.fallback.cooldownMs === 0
      if (routing.primary.cooldownMs > 0 && !fallbackReady) {
        const fallbackWait = routing.fallback?.cooldownMs ?? 0
        const wait = fallbackWait > 0 ? Math.min(routing.primary.cooldownMs, fallbackWait) : routing.primary.cooldownMs
        const detail = routing.routingMode === 'preferred' && routing.fallback && !routing.fallback.configured
          ? `${providerLabel(routing.primary.providerId)} está em cooldown e o fallback ${providerLabel(routing.fallback.providerId)} não está configurado. Configure-o ou aguarde.`
          : taskFailure('provider_unavailable', wait)
        rollback(detail, wait); busy.current = false; return false
      }
      if (routing.primary.cooldownMs > 0 && fallbackReady && routing.fallback) {
        change({ providerRoute: `${providerLabel(routing.primary.providerId)} em cooldown · ${providerLabel(routing.fallback.providerId)} elegível` })
      }
      if (id === null) id = await createSession()
      if (run !== generation.current) return false
      const sessionId = id
      change({ sessionId })
      let terminal = false
      const taskId = await startConversationTask(sessionId, message, (event) => {
        if (run !== generation.current) return
        if (event.state === 'pending' || event.state === 'running') change({ activeTaskId: event.taskId })
        if (event.type === 'provider_selected') change({ providerRoute: `Provider ativo: ${providerLabel(event.provider_id)} · tentativa ${event.attempt}` })
        if (event.type === 'provider_fallback') change({ providerRoute: `Fallback real: ${providerLabel(event.from_provider_id)} → ${providerLabel(event.to_provider_id)} · ${event.reason_code}` })
        if (event.type === 'provider_chunk') change({ preview: current.current.preview + event.chunk })
        if (event.type === 'task_result_ready') {
          const providers = event.result.usage.providersUsed.map(providerLabel).join(' → ')
          change({ providerRoute: `Resposta concluída por ${providerLabel(event.result.providerId)}${providers ? ` · rota ${providers}` : ''}` })
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
              const cooldowns = [status.primary.cooldownMs, status.fallback?.cooldownMs ?? 0].filter(value => value > 0)
              const wait = cooldowns.length > 0 ? Math.min(...cooldowns) : 0
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
      if (run !== generation.current) { await cancelTask(taskId); return false }
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
