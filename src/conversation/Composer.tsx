import { useEffect, useRef, type KeyboardEvent } from 'react'
import type { ConversationState } from './types'

type Props = { state: ConversationState; onDraft: (draft: string) => void; onSend: () => void; onCancel: () => void; onClose: () => void; onPanel: () => void; panelOpen: boolean }
export function Composer({ state, onDraft, onSend, onCancel, onClose, onPanel, panelOpen }: Props) {
  const input = useRef<HTMLTextAreaElement>(null)
  useEffect(() => { input.current?.focus() }, [])
  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); onSend() }
    if (event.key === 'Escape') { event.preventDefault(); onClose() }
  }
  return <section className="composer" aria-label="Compositor de mensagem" data-no-window-drag>
    <textarea ref={input} value={state.draft} onChange={(event) => onDraft(event.target.value)} onKeyDown={onKeyDown} placeholder="Mensagem para Luna…" aria-label="Mensagem para Luna" rows={2} />
    <div className="composer-actions">
      <span role="status">{state.assistantStreaming ? 'Luna está escrevendo…' : ''}</span>
      {!panelOpen && state.messages.length > 0 && <button type="button" onClick={onPanel}>Conversa</button>}
      {state.activeTaskId !== null && <button type="button" onClick={onCancel}>Cancelar</button>}
      <button type="button" className="send-button" disabled={!state.draft.trim() || state.assistantStreaming} onClick={onSend}>Enviar</button>
    </div>
    {state.error && <p className="conversation-error" role="alert">{state.error}</p>}
  </section>
}
