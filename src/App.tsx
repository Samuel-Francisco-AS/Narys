import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import { invoke, isTauri } from '@tauri-apps/api/core'
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
  const [pocError, setPocError] = useState<string | null>(null)
  const [pocGraphics, setPocGraphics] = useState('aguardando primeira amostra')
  const canvasBeforePoc = useRef<HTMLCanvasElement | null>(null)
  const setPocVisible = async (surface: 'composer' | 'conversation', visible: boolean) => {
    if (!isTauri()) return
    if (!canvasBeforePoc.current) canvasBeforePoc.current = document.querySelector('.scene-canvas canvas')
    try {
      await invoke('set_auxiliary_poc_visible', { surface, visible })
      setPocError(null)
      const canvas = document.querySelector<HTMLCanvasElement>('.scene-canvas canvas')
      const gl = canvas?.getContext('webgl2')
      setPocGraphics(`main ${window.innerWidth}×${window.innerHeight} · canvas ${canvas?.width ?? '?'}×${canvas?.height ?? '?'} · mesmo canvas ${canvas === canvasBeforePoc.current ? 'sim' : 'não'} · glError ${gl?.getError() ?? '?'}`)
    } catch (error) {
      setPocError(String(error))
    }
  }

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
    if (event.target instanceof Element && event.target.closest('.debug-overlay, .debug-toggle, input, textarea, button, a, [data-no-window-drag], [contenteditable="true"]')) return
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
              <section className="debug-window-ergonomics" aria-label="Superfícies auxiliares POC">
                <strong>UIP-4-FIX-2A · janelas auxiliares</strong>
                <p role="status">{pocGraphics}</p>
                <div className="debug-poc-actions">
                  <button type="button" onClick={() => void setPocVisible('composer', true)}>Show Composer POC</button>
                  <button type="button" onClick={() => void setPocVisible('composer', false)}>Hide Composer POC</button>
                  <button type="button" onClick={() => void setPocVisible('conversation', true)}>Show Conversation POC</button>
                  <button type="button" onClick={() => void setPocVisible('conversation', false)}>Hide Conversation POC</button>
                  <button type="button" onClick={() => void (async () => { await setPocVisible('composer', true); await setPocVisible('conversation', true) })()}>Show both</button>
                </div>
                {pocError && <p role="alert">{pocError}</p>}
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
