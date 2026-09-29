import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import type { ConversationState } from './types'

type Props = { state: ConversationState; visible: boolean; onExited: () => void; onDraft: (draft: string) => void; onSend: () => void; onCancel: () => void; onClose: () => void; onPanel: () => void; onSettings: () => void; panelOpen: boolean }
export function Composer({ state, visible, onExited, onDraft, onSend, onCancel, onClose, onPanel, onSettings, panelOpen }: Props) {
  const input = useRef<HTMLTextAreaElement>(null)
  const [now, setNow] = useState(Date.now())
  const cooldownSeconds = state.providerCooldownUntil === null ? 0 : Math.max(0, Math.ceil((state.providerCooldownUntil - now) / 1000))
  useEffect(() => { input.current?.focus() }, [])
  useEffect(() => {
    if (state.providerCooldownUntil === null || state.providerCooldownUntil <= Date.now()) return
    setNow(Date.now())
    const timer = window.setInterval(() => setNow(Date.now()), 1000)
    return () => window.clearInterval(timer)
  }, [state.providerCooldownUntil])
  const onKeyDown = (event: KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === 'Enter' && !event.shiftKey && !event.nativeEvent.isComposing) { event.preventDefault(); onSend() }
    if (event.key === 'Escape') { event.preventDefault(); onClose() }
  }
  return <section className={`composer ${visible ? 'is-open' : ''}`} aria-label="Compositor de mensagem" aria-hidden={!visible} data-no-window-drag
    onTransitionEnd={(event) => { if (!visible && event.target === event.currentTarget && event.propertyName === 'opacity') onExited() }}>
    <textarea ref={input} value={state.draft} onChange={(event) => onDraft(event.target.value)} onKeyDown={onKeyDown} placeholder="Mensagem para Luna…" aria-label="Mensagem para Luna" rows={2} />
    <div className="composer-actions">
      <span role="status">{state.assistantStreaming ? 'Luna está escrevendo…' : cooldownSeconds > 0 ? `Provider em cooldown: ${cooldownSeconds}s · a rota será reavaliada ao enviar` : ''}</span>
      {!panelOpen && <button type="button" onClick={onPanel}>Conversas</button>}
      <button type="button" aria-label="Configurações" title="Configurações" onClick={onSettings}>⚙</button>
      {state.activeTaskId !== null && <button type="button" onClick={onCancel}>Cancelar</button>}
      <button type="button" className="send-button" disabled={!state.draft.trim() || state.assistantStreaming} onClick={onSend}>Enviar</button>
    </div>
    {state.error && <p className="conversation-error" role="alert">{state.error}</p>}
  </section>
}
