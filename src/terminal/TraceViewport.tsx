import { useEffect, useRef, useState, useSyncExternalStore } from 'react'
import { filterTrace, TRACE_ROW_HEIGHT, virtualRange, type TraceStore } from './traceStore'
export function TraceViewport({ store }: { store: TraceStore }) {
  const snapshot = useSyncExternalStore(store.subscribe, store.getSnapshot)
  const [source, setSource] = useState(''), [task, setTask] = useState(''), [kind, setKind] = useState('')
  const [scroll, setScroll] = useState(0)
  const [follow, setFollow] = useState(true)
  const viewport = useRef<HTMLDivElement>(null)
  const rows = filterTrace(snapshot.events, source, task, kind)
  const sources = [...new Set(snapshot.events.map(e => `${e.sourceType}:${e.sourceId}`))]
  const tasks = [...new Set(snapshot.events.filter(e => e.taskId !== null).map(e => String(e.taskId)))]
  const range = virtualRange(rows.length, scroll, 480)
  useEffect(() => { if (follow && viewport.current) { viewport.current.scrollTop = viewport.current.scrollHeight; setScroll(viewport.current.scrollTop) } }, [snapshot, source, task, kind, follow])
  return <div className="terminal-activity" data-trace-updates={snapshot.updates}>
    <div className="trace-filters">
      <label>Classe <select value={kind} onChange={e => { setKind(e.target.value); setScroll(0) }}><option value="">Todos</option><option>STREAM</option><option>STATE</option><option>CRITICAL</option></select></label>
      <label>Fonte <select value={source} onChange={e => { setSource(e.target.value); setScroll(0) }}><option value="">Todas</option>{sources.map(s => <option key={s}>{s}</option>)}</select></label>
      <label>Tarefa <select value={task} onChange={e => { setTask(e.target.value); setScroll(0) }}><option value="">Todas</option>{tasks.map(t => <option key={t}>{t}</option>)}</select></label>
      <label><input type="checkbox" checked={follow} onChange={e => setFollow(e.target.checked)} /> Acompanhar</label>
    </div>
    {(snapshot.incomplete || snapshot.missing > 0n || snapshot.evicted > 0n || snapshot.liveDropped > 0n) && <p className="terminal-gap" role="status" data-trace-gap>
      {snapshot.missing.toString()} eventos anteriores indisponíveis no replay · {snapshot.evicted.toString()} fragments removidos do viewport · {snapshot.liveDropped.toString()} entregas live perdidas (recuperação via replay).</p>}
    <div ref={viewport} className="trace-viewport" role="log" aria-label="Operational Trace" onScroll={e => setScroll(e.currentTarget.scrollTop)}>
      <div style={{ height: rows.length * TRACE_ROW_HEIGHT, position: 'relative' }}>
        {rows.slice(range.first, range.end).map((e, i) => <div className={`trace-row trace-${e.class.toLowerCase()}`} data-trace-row key={e.sequence} style={{ top: (range.first + i) * TRACE_ROW_HEIGHT, height: TRACE_ROW_HEIGHT }} title={`${e.sequence}–${e.lastSequence} ${e.text}`}>
          <time>{new Date(e.observedAtUnixMs).toLocaleTimeString()}</time> <b>{e.class}</b> <span>{e.sourceType}/{e.sourceId}</span> <span>{e.code ?? e.channel ?? e.kind}</span> <span className="trace-text">{JSON.stringify(e.text).slice(1, -1)}</span>
        </div>)}
      </div>
    </div>
    <small>{rows.length} itens na janela · {snapshot.events.length} retidos · texto com escapes JSON; linha única com rolagem horizontal</small>
  </div>
}
