import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import AvatarViewport from './avatar/AvatarViewport'
import type { AnimationIntent, AnimationRequest } from './avatar/runtime/types'
import CognitionPanel from './luna/CognitionPanel'
import GeminiPanel from './luna/GeminiPanel'
import LunaCorePanel from './luna/LunaCorePanel'
import MemoryPanel from './luna/MemoryPanel'
import SecurityPanel from './security/SecurityPanel'
import { WindowController, initialWindowErgonomicsState } from './window/WindowController'

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
  const [windowState, setWindowState] = useState(initialWindowErgonomicsState)
  const windowController = useRef<WindowController | null>(null)

  useEffect(() => {
    const controller = new WindowController(setWindowState)
    windowController.current = controller
    void controller.initialize()
    return () => {
      windowController.current = null
      controller.dispose()
    }
  }, [])

  const onShellPointerDownCapture = (event: ReactPointerEvent<HTMLElement>) => {
    if (!event.altKey || event.button !== 0 || !windowController.current?.canDrag) return
    if (event.target instanceof Element && event.target.closest('.debug-overlay, .debug-toggle')) return
    event.preventDefault()
    event.stopPropagation()
    void windowController.current.startDragging()
  }
  const requestAnimation = (intent: AnimationIntent) => {
    setAnimationRequest((current) => ({ id: (current?.id ?? 0) + 1, intent }))
  }

  return (
    <main className="presence-shell" onPointerDownCapture={onShellPointerDownCapture}>
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
              <section className="debug-window-ergonomics" aria-label="Ergonomia da janela">
                <strong>Janela</strong>
                <p>Always-on-top solicitado: {windowState.alwaysOnTop ? 'on' : 'off'} · Click-through: {windowState.clickThrough ? 'on' : 'off'}</p>
                <p>Recuperação externa: {windowState.recovery === 'ready' ? 'pronta' : 'indisponível'}</p>
                <p>Posição: {windowState.position ? `${windowState.position.x}, ${windowState.position.y}` : 'indisponível'}</p>
                {windowState.error && <p role="alert">{windowState.error}</p>}
                <div className="debug-window-actions">
                  <button type="button" disabled={!windowState.available || windowState.busy} onClick={() => void windowController.current?.toggleAlwaysOnTop()}>
                    {windowState.alwaysOnTop ? 'Desligar always-on-top' : 'Ligar always-on-top'}
                  </button>
                  <button type="button" disabled={windowState.recovery !== 'ready'} title="Exige recuperação externa comprovada">
                    Click-through
                  </button>
                </div>
              </section>
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
