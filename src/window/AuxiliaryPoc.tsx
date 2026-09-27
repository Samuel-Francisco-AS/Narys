import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'

type Surface = 'composer' | 'conversation'

export function AuxiliaryPoc({ surface }: { surface: Surface }) {
  const [draft, setDraft] = useState('')
  const close = () => void invoke('set_auxiliary_poc_visible', { surface, visible: false })
  return <main className={`aux-poc aux-poc-${surface}`}>
    <div className="aux-poc-heading"><strong>{surface === 'composer' ? 'Composer POC' : 'Conversation POC'}</strong><button type="button" onClick={close} aria-label="Fechar">×</button></div>
    {surface === 'composer'
      ? <input autoFocus aria-label="Digitar no Composer POC" placeholder="Teste de teclado" value={draft} onChange={(event) => setDraft(event.target.value)} />
      : <p>Painel auxiliar para avaliar posição, foco e movimento com a Luna.</p>}
  </main>
}
