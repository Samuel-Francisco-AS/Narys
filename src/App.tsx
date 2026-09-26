import { useState } from 'react'
import AvatarViewport from './avatar/AvatarViewport'
import type { AnimationRequest } from './avatar/runtime/types'

export default function App() {
  const [animationRequest, setAnimationRequest] = useState<AnimationRequest | null>(null)
  const [sceneStatus, setSceneStatus] = useState('Preparando cena 3D…')
  const [ready, setReady] = useState(false)

  return (
    <main className="app-shell">
      <header className="topbar">
        <div className="brand-mark" aria-hidden="true">A</div>
        <div>
          <div className="eyebrow">PROTÓTIPO DESKTOP · M0-B</div>
          <h1>Assistente 3D</h1>
        </div>
        <span className="version">Candidata Luna</span>
      </header>

      <div className="workspace">
        <section className="character-panel" aria-label="Personagem 3D">
          <div className="panel-heading">
            <div>
              <span className="section-label">PERSONAGEM</span>
              <h2>Luna</h2>
            </div>
            <span className={ready ? 'live-badge ready' : 'live-badge'}>
              <span className="status-dot" />{ready ? 'Em cena' : 'Verificando'}
            </span>
          </div>

          <div className="character-stage">
            <AvatarViewport
              animationRequest={animationRequest}
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
              onClick={() => setAnimationRequest((current) => ({
                id: (current?.id ?? 0) + 1,
                intent: { type: 'greeting' },
              }))}
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
