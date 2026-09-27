import { useState } from 'react'
import AvatarViewport from './avatar/AvatarViewport'
import type { AnimationIntent, AnimationRequest } from './avatar/runtime/types'
import CognitionPanel from './luna/CognitionPanel'
import GeminiPanel from './luna/GeminiPanel'
import LunaCorePanel from './luna/LunaCorePanel'
import MemoryPanel from './luna/MemoryPanel'
import SecurityPanel from './security/SecurityPanel'

type DebugSection = 'core' | 'memory' | 'gemini' | 'cognition' | 'security'

const debugSections: { id: DebugSection; label: string }[] = [
  { id: 'core', label: 'Core' },
  { id: 'memory', label: 'Memory' },
  { id: 'gemini', label: 'Gemini' },
  { id: 'cognition', label: 'Cognition' },
  { id: 'security', label: 'Security' },
]

export default function App() {
  const [animationRequest, setAnimationRequest] = useState<AnimationRequest | null>(null)
  const [sceneStatus, setSceneStatus] = useState('Preparando cena 3D…')
  const [ready, setReady] = useState(false)
  const [debugOpen, setDebugOpen] = useState(false)
  const [debugSection, setDebugSection] = useState<DebugSection | null>(null)
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
      <div className="presence-handle" aria-hidden="true"><span>⌄</span></div>

      {import.meta.env.DEV && (
        <>
          <button
            type="button"
            className="debug-toggle"
            aria-label={debugOpen ? 'Fechar diagnósticos DEV' : 'Abrir diagnósticos DEV'}
            aria-expanded={debugOpen}
            aria-controls={debugOpen ? 'debug-overlay' : undefined}
            onClick={() => {
              if (debugOpen) setDebugSection(null)
              setDebugOpen((open) => !open)
            }}
          >
            {debugOpen ? '×' : 'DEV'}
          </button>
          {debugOpen && (
            <aside id="debug-overlay" className="debug-overlay" aria-label="Diagnósticos DEV">
              <p className="debug-scene-status" role="status">{ready ? 'WebGL ativo' : 'WebGL não pronto'} · {sceneStatus}</p>
              <nav className="debug-navigation" aria-label="Seções de diagnóstico">
                {debugSections.map(({ id, label }) => (
                  <button key={id} type="button" aria-pressed={debugSection === id} onClick={() => setDebugSection(id)}>{label}</button>
                ))}
              </nav>
              {debugSection === null && <p className="debug-placeholder">Selecione um diagnóstico.</p>}
              {debugSection === 'core' && <LunaCorePanel onAnimationIntent={requestAnimation} />}
              {debugSection === 'memory' && <MemoryPanel />}
              {debugSection === 'gemini' && <GeminiPanel onAnimationIntent={requestAnimation} />}
              {debugSection === 'cognition' && <CognitionPanel onAnimationIntent={requestAnimation} />}
              {debugSection === 'security' && <SecurityPanel />}
            </aside>
          )}
        </>
      )}
    </main>
  )
}
