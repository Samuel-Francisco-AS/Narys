import { invoke } from '@tauri-apps/api/core'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { lazy, Suspense, useEffect, useRef, useState, type CSSProperties, type PointerEvent } from 'react'
import { Composer } from '../conversation/Composer'
import { ConversationPanel } from '../conversation/ConversationPanel'
import type { useConversationController } from '../conversation/ConversationController'
import type { ConversationMode } from '../conversation/types'
import { clampLayout, type ShellLayout, type AdaptiveSnapshot } from './shellPreferences'
import './economy.css'
const TerminalWorkspace = lazy(() => import('../terminal/TerminalWorkspace'))
const OperationalSummary = lazy(() => import('./OperationalSummary'))
type View = 'home' | 'conversation' | 'relays' | 'tasks' | 'system' | 'settings'
const views: { id: View; label: string; path: string }[] = [
  { id: 'home', label: 'Terminal', path: 'M3 10 12 3 21 10v11H3Z M9 21v-8h6v8' },
  { id: 'conversation', label: 'Conversa', path: 'M3 3h18v14H8l-5 4Z' },
  { id: 'relays', label: 'Relays', path: 'M4 6h16M4 12h16M4 18h16M7 3v6M17 9v6M7 15v6' },
  { id: 'tasks', label: 'Tasks', path: 'm3 6 2 2 4-4M12 6h9m-18 6 2 2 4-4M12 12h9m-18 6 2 2 4-4M12 18h9' },
  { id: 'system', label: 'System', path: 'M3 4h18v13H3ZM8 21h8M12 17v4' },
  { id: 'settings', label: 'Settings', path: 'M4 6h16M4 12h16M4 18h16M8 3v6M16 9v6M8 15v6' },
]
type Props = { adaptive?: AdaptiveSnapshot | null; conversation: ReturnType<typeof useConversationController>; layout: ShellLayout; loaded: boolean; error: string; onLayout: (layout: ShellLayout) => void; onPresence: () => void; onDrag: () => void }
export function EconomyShell({ adaptive, conversation, layout, loaded, error, onLayout, onPresence, onDrag }: Props) {
  const [view, setView] = useState<View>('conversation')
  const [width, setWidth] = useState(window.innerWidth)
  const [draftLayout, setDraftLayout] = useState<ShellLayout | null>(null)
  const resize = useRef<{ side: 'left' | 'right'; start: number; layout: ShellLayout } | null>(null)
  const [mode, setMode] = useState<ConversationMode>('CURRENT')
  const [historyId, setHistoryId] = useState<number | null>(null)
  const [actionError, setActionError] = useState('')
  const [rightOnDemand, setRightOnDemand] = useState(false)
  useEffect(() => {
    const changed = () => setWidth(window.innerWidth)
    window.addEventListener('resize', changed)
    const shortcut = (e: KeyboardEvent) => { if (e.ctrlKey && e.shiftKey && e.code === 'Space') { e.preventDefault(); setView('conversation') } }
    window.addEventListener('keydown', shortcut)
    return () => { window.removeEventListener('resize', changed); window.removeEventListener('keydown', shortcut) }
  }, [])
  const current = draftLayout ?? layout
  const expanded = current.leftOpen && width >= 1000
  const rightVisible = current.rightOpen && (width >= 900 || rightOnDemand)
  const windowAction = async (action: 'minimize' | 'toggleMaximize' | 'close') => {
    try { if (action === 'close') await invoke('close_presentation'); else await getCurrentWindow()[action]() } catch { setActionError('Controle de janela disponível somente no aplicativo desktop.') }
  }
  const openSettings = async (command: string) => {
    try { await invoke(command) } catch { setActionError('Não foi possível abrir as configurações.') }
  }
  const move = (event: PointerEvent<HTMLButtonElement>) => {
    const r = resize.current
    if (!r) return
    const delta = (event.clientX - r.start) * (r.side === 'left' ? 1 : -1)
    setDraftLayout(clampLayout({ ...r.layout, [r.side === 'left' ? 'leftWidth' : 'rightWidth']: r.layout[r.side === 'left' ? 'leftWidth' : 'rightWidth'] + delta }))
  }
  const splitter = (side: 'left' | 'right') => <button type="button" className="shell-splitter" role="separator" aria-orientation="vertical" aria-label={`Redimensionar lateral ${side === 'left' ? 'esquerda' : 'direita'}`} aria-valuemin={side === 'left' ? 160 : 220} aria-valuemax={side === 'left' ? 320 : 360} aria-valuenow={current[side === 'left' ? 'leftWidth' : 'rightWidth']} disabled={!loaded}
    onPointerDown={event => { if (event.button !== 0) return; resize.current = { side, start: event.clientX, layout: current }; event.currentTarget.setPointerCapture(event.pointerId) }}
    onPointerMove={move} onPointerUp={event => { if (!resize.current) return; move(event); const r = resize.current; const delta = (event.clientX - r.start) * (side === 'left' ? 1 : -1); onLayout(clampLayout({ ...r.layout, [side === 'left' ? 'leftWidth' : 'rightWidth']: r.layout[side === 'left' ? 'leftWidth' : 'rightWidth'] + delta })); resize.current = null; setDraftLayout(null) }}
    onLostPointerCapture={() => { resize.current = null; setDraftLayout(null) }}
    onKeyDown={event => { if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return; event.preventDefault(); const key = side === 'left' ? 'leftWidth' : 'rightWidth'; onLayout(clampLayout({ ...layout, [key]: layout[key] + (event.key === 'ArrowRight' ? 16 : -16) * (side === 'left' ? 1 : -1) })) }} />
  const panel = <Suspense fallback={<p>Carregando operação…</p>}><OperationalSummary /></Suspense>
  return <div className="economy-shell" data-view={view}>
    <header className="shell-topbar"><div className="shell-brand" onPointerDown={event => { if (event.button === 0) onDrag() }}><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 20V4l16 16V4M4 12h16" /></svg><strong>NARYS</strong></div>
      <button type="button" disabled={!loaded} aria-expanded={rightVisible} onClick={() => { if (width < 900 && !rightVisible) { setRightOnDemand(true); if (!layout.rightOpen) onLayout({ ...layout, rightOpen: true }) } else { setRightOnDemand(false); onLayout({ ...layout, rightOpen: !rightVisible }) } }}>Operação</button>
      <div className="shell-window-controls"><button title="Encerrar Core e tarefas" onClick={() => void invoke('quit_narys').catch(() => setActionError('Não foi possível encerrar a Narys.'))}>Sair da Narys</button><button aria-label="Minimizar janela" onClick={() => void windowAction('minimize')}>—</button><button aria-label="Maximizar ou restaurar janela" onClick={() => void windowAction('toggleMaximize')}>□</button><button title="Fechar interface; Core continua. Abra Narys novamente para voltar." aria-label="Fechar interface; manter Core ativo" onClick={() => void windowAction('close')}>×</button></div>
    </header>
    <div className={`shell-layout ${expanded ? 'left-expanded' : 'left-compact'} ${rightVisible ? 'right-open' : ''} ${width < 900 ? 'narrow' : ''}`} style={{ '--left-width': `${current.leftWidth}px`, '--right-width': `${current.rightWidth}px` } as CSSProperties}>
      <aside className="shell-nav"><button type="button" className="nav-collapse" aria-label={layout.leftOpen ? 'Recolher navegação' : 'Expandir navegação'} aria-expanded={layout.leftOpen} disabled={!loaded} onClick={() => onLayout({ ...layout, leftOpen: !layout.leftOpen })}>{expanded ? '‹ Recolher' : '☰'}</button>
        <nav aria-label="Navegação principal">{views.map(v => <button key={v.id} type="button" title={v.label} aria-label={v.label} aria-current={view === v.id ? 'page' : undefined} onClick={() => setView(v.id)}><svg viewBox="0 0 24 24" aria-hidden="true"><path d={v.path} /></svg><span>{v.label}</span></button>)}</nav>
        <footer><strong>{expanded ? 'NARYS' : 'N'}</strong><small>0.1.0</small></footer>
      </aside>
      {expanded && splitter('left')}
      <section className="shell-workspace" aria-label="Workspace central">
        {adaptive && <p role="status">Presentation: {adaptive.policy}{adaptive.attention && <> · Atenção requerida <button onClick={() => void invoke('acknowledge_presentation_attention')}>Reconhecer atenção</button></>}</p>}
        {(error || actionError) && <p role="alert" className="shell-error">{error || actionError}</p>}
        {view === 'conversation' ? <div className="shell-conversation">
          <ConversationPanel state={conversation.state} mode={mode} historyId={historyId} onMode={setMode} onHistoryId={setHistoryId} visible onExited={() => {}} onClose={() => setView('home')} onNew={() => { void conversation.newConversation().then(closed => { if (closed) { setMode('CURRENT'); setHistoryId(null) } }) }} onResume={conversation.resumeConversation} />
          <Composer state={conversation.state} visible onExited={() => {}} onDraft={conversation.setDraft} onSend={() => { setMode('CURRENT'); void conversation.send() }} onCancel={conversation.cancel} onClose={() => setView('home')} onPanel={() => setView('conversation')} onSettings={() => void openSettings('open_general_settings_window')} panelOpen />
        </div> : view === 'home' ? <Suspense fallback={<p>Carregando Terminal…</p>}><TerminalWorkspace /></Suspense> : <div className="shell-view">
          <h1>{views.find(v => v.id === view)?.label}</h1>
          {view === 'relays' && <><p>Providers, credenciais e roteamento existentes.</p><button onClick={() => void openSettings('open_ai_settings_window')}>Providers e operação</button></>}
          {view === 'tasks' && <><p>Tarefa da conversa: {conversation.state.activeTaskId === null ? 'nenhuma ativa nesta Interaction' : `#${conversation.state.activeTaskId}`}</p><p>TaskGraph, Resource Allocation e Handoff podem ser inspecionados na superfície operacional existente.</p><button onClick={() => void openSettings('open_ai_settings_window')}>Abrir TaskGraph / operação</button>{conversation.state.activeTaskId !== null && <button onClick={conversation.cancel}>Cancelar tarefa da conversa</button>}<p>Aprovações de ferramentas ainda não possuem capability de produto nesta versão.</p></>}
          {view === 'system' && <><p>Presentation: Economy · superfície 3D desmontada.</p><p>Sessão: {conversation.state.sessionId ?? 'ainda não iniciada'}</p><p>{conversation.state.providerRoute ?? 'Rota ainda não observada nesta conversa.'}</p><p>CPU, RAM, uptime e detalhes do sistema: indisponíveis nesta superfície.</p><button onClick={() => void openSettings('open_ai_settings_window')}>Diagnósticos operacionais</button></>}
          {view === 'settings' && <><p>Economy é o padrão. Presence 3D é uma escolha manual persistida; usa os mesmos contratos cognitivos.</p><button disabled={!loaded} onClick={onPresence}>Ativar Presence 3D</button><button onClick={() => void openSettings('open_general_settings_window')}>Configurações gerais</button><button onClick={() => void openSettings('open_ai_settings_window')}>IA, modelos e credenciais</button></>}
        </div>}
      </section>
      {rightVisible && <>{width >= 900 && splitter('right')}<aside className="shell-operation"><button type="button" aria-label="Recolher painel operacional" disabled={!loaded} onClick={() => { setRightOnDemand(false); onLayout({ ...layout, rightOpen: false }) }}>Recolher ›</button>{panel}</aside></>}
    </div>
  </div>
}
