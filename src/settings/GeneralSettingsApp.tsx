import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import './settings.css'

export default function GeneralSettingsApp() {
  const [error, setError] = useState('')
  return <main className="settings-page">
    <header><p className="settings-kicker">LUNA · CONFIGURAÇÕES</p><h1>Configurações da Luna</h1><p>Preferências do aplicativo em janelas independentes.</p></header>
    <section className="settings-card"><h2>Geral</h2>
      <p>Janela, performance, atalhos, click-through e always-on-top.</p>
      <p>Os controles gerais editáveis chegam na UIP-6B. Os controles atuais continuam na janela principal.</p>
    </section>
    <section className="settings-card"><h2>IA e modelos</h2><p>Escolha provider, modelo e limites de cada papel cognitivo.</p>
      <button type="button" onClick={() => void invoke('open_ai_settings_window').catch(() => setError('Não foi possível abrir IA e modelos.'))}>Abrir IA e modelos</button>
    </section>
    {error && <p role="alert" className="settings-error">{error}</p>}
  </main>
}
