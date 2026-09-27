import { useEffect, useRef, useState } from 'react'
import { cancelTask, lunaCoreAvailable } from '../luna/taskClient'
import { startGeminiTask } from '../luna/geminiTaskClient'
import { createSession, geminiStatus, getSession } from './conversationClient'
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
    try {
      const status = await geminiStatus()
      if (!status.configured) throw new Error('Configure o Gemini no painel DEV antes de conversar.')
      if (id === null) id = await createSession()
      if (run !== generation.current) return false
      const sessionId = id
      change({ sessionId, draft: '', error: null, preview: '', assistantStreaming: true,
        messages: [...current.current.messages, { id: -run, sessionId, role: 'user', content: message, createdAt: '' }] })
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
            change({ assistantStreaming: false, preview: '', error: event.type === 'task_failed' ? `Falha na resposta (${event.detail}).` : 'Resposta cancelada.' })
          }
        }
      })
      if (run !== generation.current) { await cancelTask(taskId); return false }
      if (!terminal) change({ activeTaskId: taskId })
      return true
    } catch (error) {
      if (run === generation.current) change({ assistantStreaming: false, error: error instanceof Error ? error.message : 'Não foi possível iniciar a conversa.' })
      return false
    } finally {
      if (!current.current.assistantStreaming) busy.current = false
    }
  }
  const cancel = () => { if (current.current.activeTaskId !== null) void cancelTask(current.current.activeTaskId) }
  const newConversation = () => {
    if (busy.current) return false
    generation.current += 1
    change(initial)
    return true
  }
  return { state, setDraft: (draft: string) => change({ draft }), send, cancel, newConversation }
}
