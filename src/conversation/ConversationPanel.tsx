import { useEffect, useRef, useState } from 'react'
import { getHistorySession, listHistory } from './conversationClient'
import type { ConversationHistoryItem, ConversationMode, ConversationSession, ConversationState } from './types'

type Props = {
  state: ConversationState; mode: ConversationMode; historyId: number | null
  onMode: (mode: ConversationMode) => void; onHistoryId: (id: number | null) => void
  visible: boolean; onExited: () => void; onClose: () => void; onNew: () => void
}
const dateLabel = (value: string) => {
  const date = new Date(value)
  return Number.isNaN(date.getTime()) ? value : new Intl.DateTimeFormat('pt-BR', { dateStyle: 'short', timeStyle: 'short' }).format(date)
}
const errorLabel = 'Não foi possível carregar o histórico.'

export function ConversationPanel({ state, mode, historyId, onMode, onHistoryId, visible, onExited, onClose, onNew }: Props) {
  const list = useRef<HTMLDivElement>(null)
  const [history, setHistory] = useState<ConversationHistoryItem[]>([])
  const [detail, setDetail] = useState<ConversationSession | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => { if (visible && mode === 'CURRENT' && list.current) list.current.scrollTop = list.current.scrollHeight }, [visible, mode, state.messages, state.preview])
  useEffect(() => {
    if (mode !== 'HISTORY_LIST') return
    let live = true
    setLoading(true); setError(null)
    void listHistory().then((items) => { if (live) setHistory(items) })
      .catch(() => { if (live) setError(errorLabel) })
      .finally(() => { if (live) setLoading(false) })
    return () => { live = false }
  }, [mode])
  useEffect(() => {
    if (mode !== 'HISTORY_DETAIL' || historyId === null) return
    let live = true
    setDetail(null); setLoading(true); setError(null)
    void getHistorySession(historyId).then((session) => { if (live) setDetail(session) })
      .catch(() => { if (live) setError('Não foi possível abrir esta sessão.') })
      .finally(() => { if (live) setLoading(false) })
    return () => { live = false }
  }, [mode, historyId])
  const current = () => { onHistoryId(null); onMode('CURRENT') }
  const historyList = () => { onHistoryId(null); onMode('HISTORY_LIST') }
  return <aside className={`conversation-screen ${visible ? 'is-open' : ''}`} aria-label={mode === 'CURRENT' ? 'Conversa atual' : 'Histórico de conversas'} aria-hidden={!visible} data-no-window-drag
    onTransitionEnd={(event) => { if (!visible && event.target === event.currentTarget && event.propertyName === 'opacity') onExited() }}>
    <header>
      <span>{mode === 'CURRENT' ? 'Luna · conversa atual' : mode === 'HISTORY_LIST' ? 'Luna · histórico' : 'Luna · sessão'}</span>
      <div>
        {mode === 'CURRENT' ? <><button type="button" onClick={historyList} disabled={state.assistantStreaming} title={state.assistantStreaming ? 'Aguarde a resposta atual' : undefined}>Histórico</button><button type="button" onClick={onNew} disabled={state.assistantStreaming}>Nova conversa</button></>
          : <><button type="button" onClick={mode === 'HISTORY_DETAIL' ? historyList : current}>← Voltar</button>{mode === 'HISTORY_DETAIL' && <button type="button" onClick={current}>Atual</button>}</>}
        <button type="button" onClick={onClose} aria-label="Fechar painel">×</button>
      </div>
    </header>
    {mode === 'CURRENT' && <div className="conversation-messages" ref={list}>
      {state.messages.length === 0 && !state.assistantStreaming && <p className="conversation-empty">Sua conversa aparece aqui.</p>}
      {state.messages.map((message) => <article key={message.id} className={`conversation-message ${message.role}`}><strong>{message.role === 'user' ? 'Você' : 'Luna'}</strong><p>{message.content}</p></article>)}
      {state.assistantStreaming && <article className="conversation-message assistant"><strong>Luna <span className="streaming-indicator">· escrevendo</span></strong><p>{state.preview || '…'}</p></article>}
      {state.error && <p className="conversation-error" role="status">{state.error}</p>}
    </div>}
    {mode === 'HISTORY_LIST' && <div className="conversation-messages history-list">
      {loading && <p className="conversation-empty">Carregando histórico…</p>}
      {error && <p className="conversation-error" role="alert">{error}</p>}
      {!loading && !error && history.length === 0 && <p className="conversation-empty">Nenhuma conversa anterior.</p>}
      {!loading && !error && history.map((item) => <button type="button" className="history-item" key={item.id} onClick={() => { onHistoryId(item.id); onMode('HISTORY_DETAIL') }}>
        <strong>{item.title}</strong><span>{dateLabel(item.updatedAt)} · {item.messageCount} mensagens{item.status === 'active' ? ' · em andamento' : ''}</span>
        {item.preview && <small>{item.preview}</small>}
      </button>)}
    </div>}
    {mode === 'HISTORY_DETAIL' && <div className="conversation-messages history-detail">
      {loading && <p className="conversation-empty">Carregando sessão…</p>}
      {error && <p className="conversation-error" role="alert">{error}</p>}
      {detail && <><p className="history-date">{dateLabel(detail.messages[0]?.createdAt || '')} · Somente leitura</p>
        {detail.messages.map((message) => <article key={message.id} className={`conversation-message ${message.role}`}><strong>{message.role === 'user' ? 'Você' : 'Luna'}</strong><p>{message.content}</p></article>)}
      </>}
    </div>}
  </aside>
}
