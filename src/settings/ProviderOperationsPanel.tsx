import { useEffect, useRef, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { ReactNode } from 'react'
import type { ProviderAdmission } from './providerAdmission'
import type { ProviderRate } from './providerRate'
import type { ProviderResilience } from './providerResilience'
import type { ProviderTelemetry } from './providerTelemetry'
import { OperationalPoller } from './operationalPolling'
import { LocalRatePolicyEditor } from './LocalRatePolicyEditor'
import {
  capturePresentation, durationText, factOrigin, factText, numberText, operationalConditions, outcomeText, provenanceLabels,
  quotaLabels, scopeText, timingText, transitionLabels, unixText, usageLabels, unknown,
} from './providerOperational'
import type { OperationalProviderInfo, ProviderOperationalSnapshot } from './providerOperational'

function Facts({ rows }: { rows: [string, ReactNode][] }) {
  return <dl className="operations-facts">{rows.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>
}
function AdmissionDetails({ admission: a }: { admission: ProviderAdmission }) {
  return <details className="operations-details"><summary>Admission / concurrency local</summary>
    <p>Origem: Runtime Luna. Chamadas ativas incluem preflight; não representam sockets. Fila vazia não mede velocidade.</p>
    <Facts rows={[
      ['Foreground interactive na fila', numberText(a.queuedByClass.foreground_interactive)], ['Foreground task na fila', numberText(a.queuedByClass.foreground_task)], ['Background na fila', numberText(a.queuedByClass.background)],
      ['Total admissions', numberText(a.totalAdmissions)], ['Total waited', numberText(a.totalWaited)], ['Queue delay recente', durationText(a.queueDelayRecentMs)],
      ['Amostras de delay', numberText(a.queueDelaySamples)], ['Delay acumulado', durationText(a.queueDelayTotalMs)],
      ['Queue full count', numberText(a.queueFullCount)], ['Queue timeout count', numberText(a.queueTimeoutCount)], ['Counters saturated', a.countersSaturated ? 'Sim · precisão limitada' : 'Não'],
    ]} />
    {a.queueDelaySamples === 0 && <p>Sem amostras de delay de admissions que aguardaram. Não há média calculada.</p>}
  </details>
}
function ResilienceDetails({ resilience: r }: { resilience: ProviderResilience }) {
  return <details className="operations-details"><summary>Resilience / circuit breaker</summary>
    <p>Estado transitório por provider. Closed permite avaliação de uma tentativa; não garante sucesso remoto. Open com janela concluída só transita numa próxima autorização.</p>
    <Facts rows={[
      ['Falhas elegíveis / threshold local', `${numberText(r.consecutiveEligibleFailures)} / ${numberText(r.configuredThreshold)}`], ['Open restante', durationText(r.openRemainingMs)],
      ['Probes HalfOpen ativos / máximo local', `${numberText(r.halfOpenProbesActive)} / ${numberText(r.halfOpenMaxProbes)}`],
      ['Transition count', numberText(r.transitionCount)], ['Última transição', r.lastTransitionReason === null ? 'Nenhuma transição observada' : transitionLabels[r.lastTransitionReason] ?? 'Código não reconhecido'],
      ['Breaker open count', numberText(r.breakerOpenCount)], ['Half-open count', numberText(r.halfOpenCount)], ['Recovery count', numberText(r.recoveryCount)], ['Saturated', r.saturated ? 'Sim · precisão limitada' : 'Não'],
    ]} />
  </details>
}
function TelemetryDetails({ telemetry: t }: { telemetry: ProviderTelemetry }) {
  const requests = t.usage.requests.observed
  return <>
    <details className="operations-details"><summary>Uso factual observado</summary>
      <p>Uso observado desde o início deste runtime. Não representa consumo total da conta. Cada dimensão mede apenas as chamadas que a reportaram.</p>
      <div className="operations-table-wrap"><table><caption>Requests e tokens independentes</caption><thead><tr><th scope="col">Dimensão</th><th scope="col">Valor observado</th><th scope="col">Chamadas reportaram</th><th scope="col">Saturated</th></tr></thead><tbody>
        {Object.entries(usageLabels).map(([key, label]) => {
          const c = t.usage[key as keyof typeof usageLabels]
          const partial = key !== 'requests' && requests.state === 'known' && c.reportingRequests < requests.value
          return <tr key={key}><th scope="row">{label}</th><td>{factText(c.observed, numberText)}<small>{factOrigin(c.observed)}</small>{key !== 'requests' && <small>{partial ? 'Medição parcial' : 'Somente uso reportado; sem estimativa adicional'}</small>}</td><td>{numberText(c.reportingRequests)}</td><td>{c.saturated ? 'Sim · precisão limitada' : 'Não'}</td></tr>
        })}
      </tbody></table></div>
      <p>Recência da última atualização: {t.updatedAgeMs === null ? unknown : `há ${durationText(t.updatedAgeMs)}`}.</p>
      <p>Último resultado factual: {factText(t.lastOutcome, outcomeText)}. <small>{factOrigin(t.lastOutcome)}</small></p>
    </details>
    <details className="operations-details"><summary>Quotas factuais do provider</summary>
      <p>Limite, restante e reset são fatos independentes. Ausência: Não informado pelo provider. Scopes Model não se aplicam ao provider inteiro ou a outros modelos.</p>
      {t.quotas.map((q, index) => <div className="operations-table-wrap" key={index}><table><caption>{scopeText(q.scope)}</caption><thead><tr><th scope="col">Dimensão</th><th scope="col">Limite</th><th scope="col">Restante</th><th scope="col">Reset</th></tr></thead><tbody>
        {Object.entries(quotaLabels).map(([dim, label]) => {
          const f = q.dimensions[dim as keyof typeof quotaLabels]
          return <tr key={dim}><th scope="row">{label}</th><td>{factText(f.limit, numberText)}<small>Origem: {factOrigin(f.limit)}</small></td><td>{factText(f.remaining, numberText)}<small>Origem: {factOrigin(f.remaining)}</small></td><td>{factText(f.reset, timingText)}<small>Origem: {factOrigin(f.reset)}</small></td></tr>
        })}
      </tbody></table></div>)}
    </details>
  </>
}
const sourceLabels = { external_fact: 'Fato externo', local_policy: 'Policy local', daily_budget: 'DailyBudget' }
function RateDetails({ rate: r }: { rate: ProviderRate }) {
  return <details className="operations-details"><summary>Rate accounting / constraints</summary>
    <p>Consumed e reserved pertencem ao accounting operacional. Não são o ledger factual nem quotas comerciais. Saldo de tokens unresolved não é crédito utilizável conhecido.</p>
    {r.constraints.length === 0 && <p>Nenhuma constraint conhecida. Quotas externas permanecem desconhecidas quando não informadas.</p>}
    {r.constraints.map((c, index) => <fieldset key={index}><legend>{scopeText(c.scope)} · {quotaLabels[c.dimension]} · {sourceLabels[c.source]}</legend>
      <Facts rows={[
        ['Source', c.source], ['Provenance', c.provenance === null ? unknown : provenanceLabels[c.provenance]], ['Capacity', numberText(c.capacity)],
        ['Consumed', numberText(c.consumed)], ['Reserved', numberText(c.reserved)], ['Effective remaining', numberText(c.effectiveRemaining)],
        ['Reset UTC', unixText(c.resetUnixMs)], ['Reset restante', durationText(c.resetInMs)], ['Unaccounted token calls', numberText(c.unaccountedTokenCalls)], ['Saturated', c.saturated ? 'Sim · precisão limitada' : 'Não'],
      ]} />
      {c.unaccountedTokenCalls > 0 && <p className="settings-warning">Accounting unresolved: {numberText(c.unaccountedTokenCalls)} chamadas com tokens incompletos nesta constraint.</p>}
      {c.external && <p>Fatos externos retidos pelo rate manager: limite {factText(c.external.limit, numberText)} ({factOrigin(c.external.limit)}); restante {factText(c.external.remaining, numberText)} ({factOrigin(c.external.remaining)}); reset {factText(c.external.reset, timingText)} ({factOrigin(c.external.reset)}). Podem diferir da última observação em telemetria devido à reconciliação conservadora.</p>}
    </fieldset>)}
  </details>
}
function ProviderCard({ provider, telemetry: t, admission: a, rate: r, resilience: h, refresh }: {
  provider: OperationalProviderInfo; telemetry?: ProviderTelemetry; admission?: ProviderAdmission; rate?: ProviderRate; resilience?: ProviderResilience; refresh: () => Promise<boolean>
}) {
  const unresolved = r?.constraints.some(c => c.unaccountedTokenCalls > 0)
  return <article className="settings-card operations-provider" aria-labelledby={`operation-${provider.id}`}>
    <h3 id={`operation-${provider.id}`}>{provider.displayName}</h3>
    <ul className="operations-conditions">{operationalConditions(provider, a, r, h).map(text => <li key={text}>{text}</li>)}</ul>
    <p>A autorização final depende do target/modelo, da fila e dos gates no runtime. Disponível para tentativa não garante sucesso remoto.</p>
    <Facts rows={[
      ['Configurado', provider.configured ? 'Sim' : 'Não'], ['Enabled', provider.enabled ? 'Sim' : 'Não'],
      ['Circuit', h ? ({ closed: 'Closed', open: 'Open', half_open: 'HalfOpen' }[h.circuitState]) : unknown],
      ['Cooldown operacional restante', durationText(h?.cooldownRemainingMs)],
      ['Chamadas ativas / concurrency local máxima', a ? `${numberText(a.activeCalls)} / ${numberText(a.maxConcurrency)}` : unknown],
      ['Queue depth / capacidade local', a ? `${numberText(a.queueDepth)} / ${numberText(a.queueCapacity)}` : unknown],
      ['Requests observadas neste runtime', t ? factText(t.usage.requests.observed, numberText) : unknown],
      ['Última atividade', t?.updatedAgeMs == null ? unknown : `há ${durationText(t.updatedAgeMs)}`],
      ['Último Retry-After observado', t ? factText(t.retryHint, timingText) : unknown],
      ['DailyBudget local', r ? r.policy.dailyBudget === null ? 'Não configurado' : 'Configurado · detalhes em Limites locais da Luna' : unknown],
      ['Pending reservations', numberText(r?.pendingReservations)], ['Local blocks acumulados', numberText(r?.localBlocks)], ['Persistence failed', r ? r.persistenceFailed ? 'Sim' : 'Não' : unknown],
      ['Custo', 'Desconhecido · sem contrato de preço configurado'],
    ]} />
    {t?.retryHint.state === 'known' && <p>Retry-After é a última observação histórica ({factOrigin(t.retryHint)}); não significa que ainda esteja ativo. Cooldown é a decisão operacional atual.</p>}
    {r?.persistenceFailed && <p className="operations-warning">Estado local de rate não pôde ser persistido; novas operações podem ser bloqueadas por segurança.</p>}
    {unresolved && <p className="operations-warning">Accounting unresolved: existem tokens não reconciliados. Consulte as constraints; remaining pode ser Desconhecido.</p>}
    {r && (r.pendingReservations > 0 || r.localBlocks > 0) && <p className="settings-warning">Reservations pendentes: {numberText(r.pendingReservations)} · bloqueios locais acumulados: {numberText(r.localBlocks)}.</p>}
    {(r?.saturated || r?.constraints.some(c => c.saturated) || h?.saturated || a?.countersSaturated || t && Object.values(t.usage).some(c => c.saturated)) && <p className="operations-warning">Saturação numérica observada: a precisão de um ou mais contadores está limitada.</p>}
    {t && <TelemetryDetails telemetry={t} />}{a && <AdmissionDetails admission={a} />}{r && <RateDetails rate={r} />}{h && <ResilienceDetails resilience={h} />}
    {r && <LocalRatePolicyEditor rate={r} refresh={refresh} />}
  </article>
}
function OperationsRefreshControls({ snapshot, visible, manualRefreshing, refreshManual }: {
  snapshot: ProviderOperationalSnapshot | null; visible: boolean; manualRefreshing: boolean; refreshManual: () => void
}) {
  const capture = snapshot ? capturePresentation(snapshot.capturedAtUnixMs) : { label: 'Aguardando snapshot' }
  return <div className="settings-actions">
    <button type="button" disabled={!visible || manualRefreshing} onClick={refreshManual}>{manualRefreshing ? 'Atualizando…' : 'Atualizar agora'}</button>
    <span title={capture.title}>{capture.label}</span>
  </div>
}
export function ProviderOperationsPanel({ providers }: { providers: OperationalProviderInfo[] }) {
  const [snapshot, setSnapshot] = useState<ProviderOperationalSnapshot | null>(null)
  const [failed, setFailed] = useState(false)
  const [stale, setStale] = useState(false)
  const [visible, setVisible] = useState(document.visibilityState === 'visible')
  const [manualRefreshing, setManualRefreshing] = useState(false)
  const poller = useRef<OperationalPoller<ProviderOperationalSnapshot> | null>(null)
  useEffect(() => {
    // Reuse the controller through StrictMode cleanup/setup: pending IPC retains
    // its single-flight latch and cannot publish into a later lifecycle epoch.
    poller.current ??= new OperationalPoller(() => invoke<ProviderOperationalSnapshot>('get_provider_operational_snapshot'), value => { setSnapshot(value); setFailed(false); setStale(false) }, () => setFailed(true), setManualRefreshing)
    const controller = poller.current
    const visibility = () => { const shown = document.visibilityState === 'visible'; setVisible(shown); setStale(true); controller.setVisible(shown) }
    controller.start(document.visibilityState === 'visible')
    document.addEventListener('visibilitychange', visibility)
    return () => { controller.stop(); document.removeEventListener('visibilitychange', visibility) }
  }, [])
  const refresh = () => poller.current?.refresh() ?? Promise.resolve(false)
  return <section aria-labelledby="provider-operations-title" className="operations-panel">
    <h2 id="provider-operations-title">Operação dos providers</h2>
    <p>Atualização em memória a cada 1 s após a captura anterior, somente com esta janela visível. Cada autoridade é coerente internamente; a captura agregada não é uma transação global.</p>
    <OperationsRefreshControls snapshot={snapshot} visible={visible} manualRefreshing={manualRefreshing} refreshManual={() => { void poller.current?.refreshManual() }} />
    {!visible && <p>Atualização pausada · dados desatualizados enquanto a janela estiver oculta.</p>}
    {visible && stale && !failed && <p>Dados desatualizados · aguardando captura após retomar a janela.</p>}
    {failed && <p className="settings-warning" role="status">Dados desatualizados: não foi possível atualizar a operação. O último snapshot válido foi preservado.</p>}
    {providers.map(provider => <ProviderCard key={provider.id} provider={provider} telemetry={snapshot?.telemetry.find(s => s.providerId === provider.id)} admission={snapshot?.admission.find(s => s.providerId === provider.id)} rate={snapshot?.rate.find(s => s.providerId === provider.id)} resilience={snapshot?.resilience.find(s => s.providerId === provider.id)} refresh={refresh} />)}
  </section>
}
