import { useState } from 'react'
import AvatarViewport from './avatar/AvatarViewport'
import type { AnimationIntent, AnimationRequest } from './avatar/runtime/types'
import LunaCorePanel from './luna/LunaCorePanel'

export default function App() {
  const [animationRequest, setAnimationRequest] = useState<AnimationRequest | null>(null)
  const [sceneStatus, setSceneStatus] = useState('Preparando cena 3D…')
  const [ready, setReady] = useState(false)
  const [debugOpen, setDebugOpen] = useState(false)
  const requestAnimation = (intent: AnimationIntent) => {
    setAnimationRequest((current) => ({ id: (current?.id ?? 0) + 1, intent }))
  }

  return (
    <main className="presence-shell">
      <section className="character-stage" aria-label="Personagem 3D Luna">
        <AvatarViewport
          animationRequest={animationRequest}
          onStatusChange={setSceneStatus}
          onReadyChange={setReady}
        />
      </section>
      <div className="presence-handle" aria-hidden="true">⌄</div>

      {import.meta.env.DEV && (
        <>
          <button
            type="button"
            className="debug-toggle"
            aria-label={debugOpen ? 'Fechar diagnósticos DEV' : 'Abrir diagnósticos DEV'}
            aria-expanded={debugOpen}
            aria-controls={debugOpen ? 'debug-overlay' : undefined}
            onClick={() => setDebugOpen((open) => !open)}
          >
            {debugOpen ? '×' : 'DEV'}
          </button>
          {debugOpen && (
            <aside id="debug-overlay" className="debug-overlay" aria-label="Diagnósticos DEV">
              <p className="debug-scene-status" role="status">{ready ? 'WebGL ativo' : 'WebGL não pronto'} · {sceneStatus}</p>
              <LunaCorePanel onAnimationIntent={requestAnimation} />
            </aside>
          )}
        </>
      )}
    </main>
  )
}
