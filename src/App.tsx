import { useState } from 'react'
import CharacterScene from './components/CharacterScene'

export default function App() {
  const [waveSignal, setWaveSignal] = useState(0)
  const [sceneStatus, setSceneStatus] = useState('Preparando cena 3D…')
  const [ready, setReady] = useState(false)

  return (
    <main className="app-shell">
      <header className="topbar">
        <div className="brand-mark" aria-hidden="true">A</div>
        <div>
          <div className="eyebrow">PROTÓTIPO DESKTOP · M0-A</div>
          <h1>Assistente 3D</h1>
        </div>
        <span className="version">Validação gráfica</span>
      </header>

      <div className="workspace">
        <section className="character-panel" aria-label="Personagem 3D">
          <div className="panel-heading">
            <div>
              <span className="section-label">PERSONAGEM</span>
              <h2>Presença visual</h2>
            </div>
            <span className={ready ? 'live-badge ready' : 'live-badge'}>
              <span className="status-dot" />{ready ? 'Em cena' : 'Verificando'}
            </span>
          </div>

          <div className="character-stage">
            <CharacterScene
              waveSignal={waveSignal}
              onStatusChange={setSceneStatus}
              onReadyChange={setReady}
            />
            <div className="stage-caption">CLIQUE NA PERSONAGEM PARA INTERAGIR</div>
          </div>

          <div className="character-toolbar">
            <div className="scene-status" role="status">{sceneStatus}</div>
            <button
              type="button"
              className="primary-button"
              disabled={!ready}
              onClick={() => setWaveSignal((value) => value + 1)}
            >
              <span aria-hidden="true">✳</span> Acenar
            </button>
          </div>
        </section>

        <aside className="conversation-panel" aria-label="Espaço reservado para conversa">
          <div className="panel-heading">
            <div>
              <span className="section-label">CONVERSAÇÃO</span>
              <h2>Área reservada</h2>
            </div>
          </div>
          <div className="conversation-empty">
            <div className="conversation-icon" aria-hidden="true">•••</div>
            <h3>Um espaço para conversar</h3>
            <p>A conversa será construída em um próximo checkpoint. Aqui validamos a presença e a animação da personagem.</p>
          </div>
          <div className="conversation-placeholder" aria-hidden="true">
            <span>Campo de mensagem futuro</span><span>→</span>
          </div>
        </aside>
      </div>
    </main>
  )
}
