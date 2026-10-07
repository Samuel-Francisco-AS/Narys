import { invoke } from '@tauri-apps/api/core'
import { useEffect, useRef, useState } from 'react'
import { OperationalPoller } from '../settings/operationalPolling'
import type { ProviderOperationalSnapshot } from '../settings/providerOperational'
export default function OperationalSummary() {
  const [snapshot, setSnapshot] = useState<ProviderOperationalSnapshot | null>(null)
  const [stale, setStale] = useState(true)
  const poller = useRef<OperationalPoller<ProviderOperationalSnapshot> | null>(null)
  useEffect(() => {
    poller.current ??= new OperationalPoller(() => invoke('get_provider_operational_snapshot'), value => { setSnapshot(value); setStale(false) }, () => setStale(true), () => {})
    const controller = poller.current
    const visibility = () => { setStale(true); controller.setVisible(document.visibilityState === 'visible') }
    controller.start(document.visibilityState === 'visible')
    document.addEventListener('visibilitychange', visibility)
    return () => { controller.stop(); document.removeEventListener('visibilitychange', visibility) }
  }, [])
  return <section className="operational-summary" aria-label="Snapshot operacional">
    <h2>Operação</h2><p className="muted">Fonte: Scheduler / snapshot do Core. Atualização até 1 Hz enquanto visível.</p>
    {stale && <p role="status">{snapshot ? 'Último snapshot desatualizado.' : 'Snapshot indisponível ou aguardando captura.'}</p>}
    {snapshot && <>
      <p>Captura: {snapshot.capturedAtUnixMs === null ? 'indisponível' : new Date(snapshot.capturedAtUnixMs).toLocaleTimeString('pt-BR')}</p>
      {snapshot.admission.map(a => <article key={a.providerId}><h3>{a.providerId}</h3><dl>
        <dt>Chamadas ativas</dt><dd>{a.activeCalls} / {a.maxConcurrency}</dd>
        <dt>Fila local</dt><dd>{a.queueDepth} / {a.queueCapacity}</dd>
        <dt>Circuito</dt><dd>{snapshot.resilience.find(r => r.providerId === a.providerId)?.circuitState ?? 'indisponível'}</dd>
      </dl></article>)}
      {snapshot.admission.length === 0 && <p>Nenhum provider reportado nesta captura.</p>}
    </>}
  </section>
}
