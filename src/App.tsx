import { useState } from 'react'
import AvatarViewport from './avatar/AvatarViewport'
import type { AnimationIntent, AnimationRequest } from './avatar/runtime/types'
import LunaCorePanel from './luna/LunaCorePanel'

export default function App() {
  const [animationRequest, setAnimationRequest] = useState<AnimationRequest | null>(null)
  const [sceneStatus, setSceneStatus] = useState('Preparando cena 3D…')
  const [ready, setReady] = useState(false)
  const requestAnimation = (intent: AnimationIntent) => {
    setAnimationRequest((current) => ({ id: (current?.id ?? 0) + 1, intent }))
  }

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
              onClick={() => requestAnimation({ type: 'greeting' })}
            >
              <span aria-hidden="true">✳</span> Acenar
            </button>
          </div>
        </section>

        <LunaCorePanel onAnimationIntent={requestAnimation} />
      </div>
    </main>
  )
}
