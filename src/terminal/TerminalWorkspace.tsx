import { Channel, invoke, isTauri } from '@tauri-apps/api/core'
import { useEffect, useRef, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import '@xterm/xterm/css/xterm.css'
import './terminal.css'
import { decodePtyFrame, InputBatcher, ResizeCoalescer, type Session, type TraceBatch } from './protocol'
import { TraceStore } from './traceStore'
import { TraceViewport } from './TraceViewport'

export default function TerminalWorkspace() {
  const host = useRef<HTMLDivElement>(null)
  const input = useRef<InputBatcher | null>(null)
  const [session, setSession] = useState<Session | null>(null)
  const [connection, setConnection] = useState('consultando')
  const [error, setError] = useState('')
  const [ptyGap, setPtyGap] = useState('')
  const [revision, setRevision] = useState(0)
  const [activityOpen, setActivityOpen] = useState(true)
  const activityVisible = useRef(true)
  const activityControl = useRef<((enabled: boolean) => void) | null>(null)
  const [narrowTab, setNarrowTab] = useState<'shell' | 'activity'>('shell')
  const [busy, setBusy] = useState(false)
  const [store, setStore] = useState(() => new TraceStore())
  useEffect(() => { activityVisible.current = activityOpen; activityControl.current?.(activityOpen) }, [activityOpen])
  useEffect(() => {
    if (!isTauri()) { setConnection('indisponível'); return }
    let disposed = false, attachment = '', term: Terminal | undefined, observer: ResizeObserver | undefined, resize: ResizeCoalescer | undefined
    let running = false, measure = () => {}
    let trace = new TraceStore(), traceEpoch = 0n; setStore(trace); setError(''); setPtyGap(''); setConnection('consultando')
    const fail = (message: string) => { if (!disposed) setError(message) }
    const ack = (stream: string, cursor: string, epoch?: string) => invoke('acknowledge_terminal_batch', { attachmentId: attachment, stream, cursor, traceEpoch: epoch }).catch(e => { if (stream === 'trace' && String(e) === 'stale_trace_ack') return; if (!disposed) { setConnection('desconectado'); fail(`Attachment interrompida: ${String(e)}. Reconecte para recuperar o replay.`) } })
    let ready: () => void = () => {}
    const tokenReady = new Promise<void>(resolve => { ready = resolve })
    const pty = new Channel<ArrayBuffer>()
    pty.onmessage = buffer => {
      void tokenReady.then(() => {
        if (disposed || !term) return
        try {
          const frame = decodePtyFrame(buffer)
          if (frame.missingChunks > 0n) setPtyGap(`${frame.missingChunks.toString()} chunks de PTY anteriores indisponíveis; ${frame.droppedBytes.toString()} bytes descartados da retenção. Replay parcial pode começar no meio de uma sequência ANSI.`)
          // Imperative, raw Uint8Array, parser callback grants next native batch.
          // No output bytes in React state, no setState per character/chunk.
          if (frame.bytes.length) term.write(frame.bytes, () => { if (!disposed) void ack('pty', frame.cursor) })
          else void ack('pty', frame.cursor)
        } catch (e) { fail(String(e)) }
      })
    }
    const status = new Channel<Session>()
    status.onmessage = dto => { if (!disposed) { running = dto.state === 'running'; setSession(dto); if (running) measure() } }
    const activity = new Channel<TraceBatch>()
    activity.onmessage = batch => { void tokenReady.then(() => {
      if (disposed || !activityVisible.current) return
      const epoch = BigInt(batch.deliveryEpoch ?? '0')
      if (epoch < traceEpoch) return
      if (epoch > traceEpoch) { trace.dispose(); trace = new TraceStore(); setStore(trace); traceEpoch = epoch }
      trace.ingest(batch, () => { if (!disposed && activityVisible.current && epoch === traceEpoch) void ack('trace', batch.cursor, epoch.toString()) })
    }) }
    // One operation at a time, coalescing rapid toggles into the latest desired
    // state. No polling or queue of toggle requests; PTY/attachment stay intact.
    let desiredActivity = true, actualActivity = true, activityBusy = false
    const syncActivity = async () => {
      if (activityBusy) return
      activityBusy = true
      try {
        while (!disposed && desiredActivity !== actualActivity) {
          const enabled = desiredActivity
          const epoch = await invoke<string>('set_terminal_activity', { attachmentId: attachment, enabled })
          actualActivity = enabled
          if (BigInt(epoch) > traceEpoch) { traceEpoch = BigInt(epoch); trace.dispose(); trace = new TraceStore(); if (!disposed) setStore(trace) }
        }
      } catch (e) { desiredActivity = actualActivity; fail(`Activity interrompida: ${String(e)}. Reconecte para recuperar o replay.`) }
      finally { activityBusy = false }
    }
    const connect = async () => {
      try {
        const existing = await invoke<Session | null>('terminal_session_status')
        if (disposed) return
        setSession(existing); running = existing?.state === 'running'
        if (existing && host.current) {
          term = new Terminal({ cursorBlink: false, scrollback: 2000, fontSize: 13, fontFamily: 'monospace', theme: { background: '#11151e', foreground: '#e5e8f0', cursor: '#b8acf5' }, allowProposedApi: false })
          const fit = new FitAddon(); term.loadAddon(fit); term.open(host.current)
          input.current = new InputBatcher(bytes => invoke('send_terminal_input', bytes, { headers: { 'x-terminal-attachment': attachment } }), fail)
          term.onData(data => input.current?.text(data)); term.onBinary(data => input.current?.binary(data))
          resize = new ResizeCoalescer((rows, cols) => invoke('resize_terminal', { attachmentId: attachment, rows, cols }), fail)
          measure = () => { if (!disposed && running && attachment && host.current?.clientHeight && host.current.clientWidth) { fit.fit(); if (term) resize?.resize(term.rows, term.cols) } }
          observer = new ResizeObserver(measure); observer.observe(host.current)
          const result = await invoke<{ attachmentId: string }>('attach_terminal_surface', { sessionId: existing.sessionId, pty, status, activity })
          attachment = result.attachmentId; ready()
          if (!disposed) { measure(); term.focus() }
        } else {
          const result = await invoke<{ attachmentId: string }>('attach_terminal_surface', { sessionId: null, pty, status, activity })
          attachment = result.attachmentId; ready()
        }
        if (disposed) { void invoke('detach_terminal_surface', { attachmentId: attachment }); return }
        activityControl.current = enabled => { desiredActivity = enabled; if (!enabled) trace.dispose(); void syncActivity() }
        activityControl.current(activityVisible.current)
        setConnection('conectado')
      } catch (e) { ready(); if (!disposed) { setConnection('desconectado'); fail(String(e)) } }
    }
    void connect()
    return () => {
      disposed = true; activityControl.current = null; ready(); observer?.disconnect(); resize?.dispose(); input.current?.dispose(); input.current = null
      trace.dispose(); term?.dispose()
      if (attachment) void invoke('detach_terminal_surface', { attachmentId: attachment }).catch(() => {})
    }
  }, [revision])
  const start = async () => {
    setBusy(true)
    try { await invoke('open_human_terminal'); setRevision(v => v + 1) } catch (e) { setError(String(e)) } finally { setBusy(false) }
  }
  const close = async () => {
    if (!session) return
    setBusy(true)
    try { await invoke('close_human_terminal', { sessionId: session.sessionId }) } catch (e) { setError(String(e)) } finally { setBusy(false) }
  }
  const active = session?.state === 'running' || session?.state === 'starting'
  return <div className={`terminal-workspace activity-${activityOpen ? 'open' : 'closed'} narrow-${narrowTab}`} data-terminal-session={session?.sessionId ?? ''} data-terminal-state={session?.state ?? 'absent'} data-terminal-connection={connection}>
    <header className="terminal-header"><div><strong>Terminal local</strong> · {session ? `${session.shell} · ${session.state} · #${session.sessionId}` : 'sem sessão'} <small>{connection}</small></div>
      {active && <button disabled={busy} onClick={() => void close()} title="Encerrar o shell; sair desta view preserva a sessão">Encerrar sessão</button>}
      <button disabled={busy || !isTauri()} onClick={() => setRevision(v => v + 1)}>Reconectar</button>
    </header>
    <p className="terminal-note">Fechar a interface ou trocar de view preserva o shell. Sair da Narys encerra o Core e a sessão.</p>
    {error && <p className="terminal-error" role="alert">{error} <button onClick={() => { input.current?.resume(); setError('') }}>Retomar entrada</button></p>}
    {ptyGap && <p className="terminal-gap" role="status" data-pty-gap>{ptyGap}</p>}
    <div className="terminal-mobile-tabs"><button aria-pressed={narrowTab === 'shell'} onClick={() => setNarrowTab('shell')}>Shell</button><button aria-pressed={narrowTab === 'activity'} onClick={() => { setActivityOpen(true); setNarrowTab('activity') }}>Activity</button></div>
    <section className="terminal-shell-area" aria-label="Shell humano local">
      {!active && <div className="terminal-start"><p>{session ? `Sessão ${session.state}${session.exitCode !== null ? ` · exit ${session.exitCode}` : ''}` : 'Nenhum shell foi iniciado.'}</p><button disabled={busy || !isTauri()} onClick={() => void start()}>Iniciar terminal local</button>{!isTauri() && <p>PTY disponível somente na Narys desktop.</p>}</div>}
      <div className="terminal-emulator" ref={host} />
    </section>
    <button className="terminal-activity-toggle" aria-expanded={activityOpen} onClick={() => setActivityOpen(v => !v)}>Activity {activityOpen ? '▾ Recolher' : '▸ Abrir'}</button>
    {activityOpen && <TraceViewport store={store} />}
  </div>
}
