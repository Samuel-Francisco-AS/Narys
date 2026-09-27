import { useEffect, useRef, useState } from 'react'
import { cancelTask, lunaCoreAvailable } from '../luna/taskClient'
import { startGeminiTask } from '../luna/geminiTaskClient'
import { closeSession, createSession, geminiStatus, getSession } from './conversationClient'
import type { ConversationState } from './types'

const initial: ConversationState = { sessionId: null, messages: [], draft: '', preview: '', assistantStreaming: false, activeTaskId: null, error: null }

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
    change({ draft: '', error: null, preview: '', assistantStreaming: true,
      messages: [...current.current.messages, { id: -run, sessionId: id ?? 0, role: 'user', content: message, createdAt: '' }] })
    const rollback = (error: string) => change({ draft: message, assistantStreaming: false, preview: '', activeTaskId: null,
      messages: current.current.messages.filter((item) => item.id !== -run), error })
    try {
      const status = await geminiStatus()
      if (!status.configured) throw new Error('Configure o Gemini no painel DEV antes de conversar.')
      if (id === null) id = await createSession()
      if (run !== generation.current) return false
      const sessionId = id
      change({ sessionId })
      let terminal = false
      const taskId = await startGeminiTask(sessionId, message, (event) => {
        if (run !== generation.current) return
        if (event.state === 'pending' || event.state === 'running') change({ activeTaskId: event.taskId })
        if (event.type === 'provider_chunk') change({ preview: current.current.preview + event.chunk })
        if (event.type === 'task_completed' || event.type === 'task_cancelled' || event.type === 'task_failed') {
          terminal = true
          change({ activeTaskId: null })
          if (event.type === 'task_completed') {
            void refresh(sessionId, run)
              .then(() => { if (run === generation.current) { busy.current = false; change({ assistantStreaming: false, error: null }) } })
              .catch(() => { if (run === generation.current) { busy.current = false; change({ assistantStreaming: false, preview: '', error: 'Resposta concluída; não foi possível atualizar a conversa local.' }) } })
          } else {
            busy.current = false
            rollback(event.type === 'task_failed' ? `Falha na resposta (${event.detail}). Mensagem não enviada.` : 'Resposta cancelada. Mensagem não enviada.')
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
  return { state, setDraft: (draft: string) => change({ draft }), send, cancel, newConversation }
}
