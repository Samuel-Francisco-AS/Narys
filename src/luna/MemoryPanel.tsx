import { useCallback, useEffect, useState } from 'react'
import { invoke, isTauri } from '@tauri-apps/api/core'

type Status = {
  databaseAvailable: boolean
  errorCode: string | null
  identityName: string | null
  identityVersion: string | null
  memoryCount: number
  conversationCount: number
  taskCount: number
  memoryTitles: string[]
}
type Conversation = { id: number; messages: { role: string; content: string }[] }

export default function MemoryPanel() {
  const [status, setStatus] = useState<Status | null>(null)
  const [conversation, setConversation] = useState<Conversation | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const available = isTauri()
  const refresh = useCallback(async () => {
    if (!available) return
    try {
      const [next, recent] = await Promise.all([
        invoke<Status>('lr4_status'),
        import.meta.env.DEV ? invoke<Conversation | null>('lr4_get_recent_conversation') : Promise.resolve(null),
      ])
      setStatus(next)
      setConversation(recent)
      setError(null)
    } catch {
      setError('Falha ao consultar diagnóstico LR-4.')
    }
  }, [available])
  useEffect(() => {
    void refresh()
    const onTask = () => { window.setTimeout(() => void refresh(), 350) }
    window.addEventListener('lr4-task-terminal', onTask)
    return () => window.removeEventListener('lr4-task-terminal', onTask)
  }, [refresh])
  async function run(command: 'lr4_import_private_bootstrap' | 'lr4_create_diagnostic_conversation') {
    setBusy(true)
    setError(null)
    try { await invoke(command); await refresh() }
    catch (cause) { setError(typeof cause === 'string' ? cause : 'Falha na operação diagnóstica.') }
    finally { setBusy(false) }
  }
  return <section className="security-panel" aria-label="Diagnóstico de persistência LR-4">
    <strong>MEMORY · LR-4</strong>
    {!available && <span>Disponível somente na janela Tauri.</span>}
    {available && <>
      <span>Database: {status?.databaseAvailable ? 'disponível' : status ? `indisponível (${status.errorCode ?? 'erro'})` : 'consultando'}</span>
      <span>Identity: {status?.identityName ?? '—'}</span>
      <span>Version: {status?.identityVersion ?? '—'}</span>
      <span>Memórias: {status?.memoryCount ?? '—'} · Conversas: {status?.conversationCount ?? '—'} · Tarefas históricas: {status?.taskCount ?? '—'}</span>
      {status?.memoryTitles && status.memoryTitles.length > 0 && <ul className="memory-titles">{status.memoryTitles.map(title => <li key={title}>{title}</li>)}</ul>}
      {conversation && <span>Última conversa diagnóstica: #{conversation.id} · {conversation.messages.length} mensagens</span>}
      {import.meta.env.DEV && <div className="luna-core-controls">
        <button type="button" className="secondary-button" disabled={busy || !status?.databaseAvailable} onClick={() => void run('lr4_import_private_bootstrap')}>Importar bootstrap privado</button>
        <button type="button" className="secondary-button" disabled={busy || !status?.databaseAvailable} onClick={() => void run('lr4_create_diagnostic_conversation')}>Criar conversa diagnóstica</button>
        <button type="button" className="secondary-button" disabled={busy} onClick={() => void refresh()}>Atualizar</button>
      </div>}
      {error && <span className="luna-core-error" role="alert">{error}</span>}
    </>}
  </section>
}
