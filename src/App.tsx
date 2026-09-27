import { useEffect, useRef, useState, type PointerEvent as ReactPointerEvent } from 'react'
import AvatarViewport from './avatar/AvatarViewport'
import type { AnimationIntent, AnimationRequest } from './avatar/runtime/types'
import CognitionPanel from './luna/CognitionPanel'
import GeminiPanel from './luna/GeminiPanel'
import LunaCorePanel from './luna/LunaCorePanel'
import MemoryPanel from './luna/MemoryPanel'
import SecurityPanel from './security/SecurityPanel'
import { WindowController, initialWindowErgonomicsState } from './window/WindowController'
import { useConversationController } from './conversation/ConversationController'
import { Composer } from './conversation/Composer'
import { ConversationPanel } from './conversation/ConversationPanel'

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
  const conversation = useConversationController()
  const [composerOpen, setComposerOpen] = useState(false)
  const [composerMounted, setComposerMounted] = useState(false)
  const [panelOpen, setPanelOpen] = useState(false)
  const [panelMounted, setPanelMounted] = useState(false)
  const composerVisible = useRef(false)
  const panelVisible = useRef(false)
  const panelPresent = useRef(false)
  const layoutGeneration = useRef(0)
  const sendStartedAt = useRef<number | null>(null)

  const openPanel = () => {
    const generation = ++layoutGeneration.current
    panelPresent.current = true
    setPanelMounted(true)
    composerVisible.current = true
    setComposerMounted(true)
    setComposerOpen(true)
    void windowController.current?.setLayout('conversation').then((ok) => {
      if (generation !== layoutGeneration.current) return
      if (!ok) { panelPresent.current = false; setPanelMounted(false); sendStartedAt.current = null; return }
      if (import.meta.env.DEV && sendStartedAt.current !== null) console.debug(`[UIP-4-FIX] Enter→setSize ${Math.round(performance.now() - sendStartedAt.current)}ms`)
      requestAnimationFrame(() => {
        if (generation !== layoutGeneration.current) return
        panelVisible.current = true
        setPanelOpen(true)
        if (import.meta.env.DEV && sendStartedAt.current !== null) {
          console.debug(`[UIP-4-FIX] Enter→primeiro frame do painel ${Math.round(performance.now() - sendStartedAt.current)}ms`)
          sendStartedAt.current = null
        }
      })
    })
  }
  const closePanel = (keepComposer = composerVisible.current) => {
    ++layoutGeneration.current
    panelVisible.current = false
    setPanelOpen(false)
    if (!keepComposer) { composerVisible.current = false; setComposerOpen(false) }
    if (!panelPresent.current) void windowController.current?.setLayout(keepComposer ? 'composer' : 'presence')
    else if (!panelVisible.current && !panelOpen) onPanelExited()
  }
  const onPanelExited = () => {
    if (panelVisible.current) return
    panelPresent.current = false
    setPanelMounted(false)
    void windowController.current?.setLayout(composerVisible.current ? 'composer' : 'presence')
  }
  const onComposerExited = () => {
    if (composerVisible.current) return
    setComposerMounted(false)
    if (!panelPresent.current) void windowController.current?.setLayout('presence')
  }
  const toggleComposer = () => {
    if (composerVisible.current) {
      ++layoutGeneration.current
      composerVisible.current = false
      setComposerOpen(false)
      if (panelPresent.current) closePanel(false)
    } else {
      const generation = ++layoutGeneration.current
      composerVisible.current = true
      setComposerMounted(true)
      void windowController.current?.setLayout(panelPresent.current ? 'conversation' : 'composer').then((ok) => {
        if (generation !== layoutGeneration.current) return
        if (!ok) { composerVisible.current = false; setComposerMounted(false); return }
        requestAnimationFrame(() => { if (generation === layoutGeneration.current) setComposerOpen(true) })
      })
    }
  }
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.shiftKey && event.code === 'Space') {
        event.preventDefault(); toggleComposer()
      }
    }
    window.addEventListener('keydown', onKeyDown)
    return () => window.removeEventListener('keydown', onKeyDown)
  })

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
      <button type="button" className="presence-handle" aria-label={composerOpen ? 'Recolher compositor' : 'Abrir compositor'} aria-expanded={composerOpen} onClick={toggleComposer}><span>⌄</span></button>
      {composerMounted && <Composer state={conversation.state} visible={composerOpen} onExited={onComposerExited} onDraft={conversation.setDraft} onSend={() => { sendStartedAt.current = panelVisible.current ? null : performance.now(); void conversation.send(); openPanel() }} onCancel={conversation.cancel} onClose={toggleComposer} onPanel={openPanel} panelOpen={panelOpen} />}
      {panelMounted && <ConversationPanel state={conversation.state} visible={panelOpen} onExited={onPanelExited} onClose={() => closePanel()} onNew={() => { void conversation.newConversation().then((closed) => { if (closed) closePanel() }) }} />}

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
