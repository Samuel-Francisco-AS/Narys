import { useEffect, useRef, type KeyboardEvent } from 'react'
import type { ConversationState } from './types'

type Props = { state: ConversationState; visible: boolean; onExited: () => void; onDraft: (draft: string) => void; onSend: () => void; onCancel: () => void; onClose: () => void; onPanel: () => void; onSettings: () => void; panelOpen: boolean }
export function Composer({ state, visible, onExited, onDraft, onSend, onCancel, onClose, onPanel, onSettings, panelOpen }: Props) {
  const input = useRef<HTMLTextAreaElement>(null)
  useEffect(() => { input.current?.focus() }, [])
  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); onSend() }
    if (event.key === 'Escape') { event.preventDefault(); onClose() }
  }
  return <section className={`composer ${visible ? 'is-open' : ''}`} aria-label="Compositor de mensagem" aria-hidden={!visible} data-no-window-drag
    onTransitionEnd={(event) => { if (!visible && event.target === event.currentTarget && event.propertyName === 'opacity') onExited() }}>
    <textarea ref={input} value={state.draft} onChange={(event) => onDraft(event.target.value)} onKeyDown={onKeyDown} placeholder="Mensagem para Luna…" aria-label="Mensagem para Luna" rows={2} />
    <div className="composer-actions">
      <span role="status">{state.assistantStreaming ? 'Luna está escrevendo…' : ''}</span>
      {!panelOpen && <button type="button" onClick={onPanel}>Conversas</button>}
      <button type="button" aria-label="Configurações" title="Configurações" onClick={onSettings}>⚙</button>
      {state.activeTaskId !== null && <button type="button" onClick={onCancel}>Cancelar</button>}
      <button type="button" className="send-button" disabled={!state.draft.trim() || state.assistantStreaming} onClick={onSend}>Enviar</button>
    </div>
    {state.error && <p className="conversation-error" role="alert">{state.error}</p>}
  </section>
}
