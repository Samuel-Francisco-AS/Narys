import { useEffect, useRef } from 'react'
import type { ConversationState } from './types'

type Props = { state: ConversationState; visible: boolean; onClose: () => void; onNew: () => void }
export function ConversationPanel({ state, visible, onClose, onNew }: Props) {
  const list = useRef<HTMLDivElement>(null)
  useEffect(() => { if (visible && list.current) list.current.scrollTop = list.current.scrollHeight }, [visible, state.messages, state.preview])
  return <aside className={`conversation-screen ${visible ? 'is-open' : ''}`} aria-label="Conversa atual" aria-hidden={!visible} data-no-window-drag>
    <header><span>Luna · conversa atual</span><div><button type="button" onClick={onNew} disabled={state.assistantStreaming}>Nova conversa</button><button type="button" onClick={onClose} aria-label="Fechar painel">×</button></div></header>
    <div className="conversation-messages" ref={list}>
      {state.messages.length === 0 && !state.assistantStreaming && <p className="conversation-empty">Sua conversa aparece aqui.</p>}
      {state.messages.map((message) => <article key={message.id} className={`conversation-message ${message.role}`}><strong>{message.role === 'user' ? 'Você' : 'Luna'}</strong><p>{message.content}</p></article>)}
      {state.assistantStreaming && <article className="conversation-message assistant"><strong>Luna <span className="streaming-indicator">· escrevendo</span></strong><p>{state.preview || '…'}</p></article>}
      {state.error && <p className="conversation-error" role="status">{state.error}</p>}
    </div>
  </aside>
}
