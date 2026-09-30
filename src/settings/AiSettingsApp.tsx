import { useEffect, useRef, useState } from 'react'
import { Channel, invoke } from '@tauri-apps/api/core'
import './settings.css'

type Thinking = 'low' | 'medium' | 'high' | null
type Role = 'conversation' | 'summary' | 'orchestrator'
type Routing = 'fixed' | 'preferred' | 'auto'
type Target = { providerId: string; model: string; thinkingLevel: Thinking }
type Policy = { role: Role; routingMode: Routing; targets: Target[]; maxOutputTokens: number | null; maxProviderCalls: number; retryEnabled: boolean; maxRetries: number; retryBackoffMs: number; historyMaxMessages: number; historyMaxBytes: number; summaryInputMaxBytes: number; contextMaxBytes: number }
type Timeouts = { requestTimeoutMs: number; streamIdleTimeoutMs: number }
type ProviderInfo = { id: string; displayName: string; configured: boolean; enabled: boolean; capabilities: { textGeneration: boolean; streaming: boolean }; supportedThinkingLevels: Thinking[]; defaultModel: string | null }
type Settings = { providerTimeouts: Record<string, Timeouts>; providers: ProviderInfo[]; roles: Policy[]; credentialStoreAvailable: boolean }
type ProbeEvent = { type: 'selected'; providerId: string; attempt: number } | { type: 'chunk'; text: string }
type ProbeResult = { text: string; providerId: string; usage: { providerCalls: number; inputTokens: number; outputTokens: number; totalTokens: number | null; thoughtTokens: number | null; retries: number; fallbacks: number } }
type CodexRuntimeStatus = { installed: boolean; version: string | null; authenticated: boolean; authKind: 'chatgpt' | 'api_key' | 'other' | 'unknown' | 'none'; available: boolean; diagnosticCode: 'codex_not_installed' | 'codex_not_authenticated' | 'codex_status_timeout' | 'codex_status_failed' | 'codex_status_unrecognized' | null }
type CodexAppServerProbe = { launched: boolean; initialized: boolean; platformFamily: 'unix' | 'windows' | null; platformOs: 'linux' | 'macos' | 'windows' | null; diagnosticCode: string | null }
type PlanV1 = { version: 1; objective: string; steps: { id: string; description: string; requiredCapabilities: string[]; dependsOn: string[] }[]; risks: string[]; needsUserInput: boolean; questions: string[] }
type OrchestratorResult = { providerId: string; plan: PlanV1; usage: { providerCalls: number; outputTokens: number; retries: number; fallbacks: number } }
type OrchestratorEvent = { taskId: number; sequence: number; state: 'pending' | 'running' | 'completed' | 'cancelled' | 'failed'; type: 'task_started' | 'provider_selected' | 'provider_retry' | 'provider_fallback' | 'provider_output_observed' | 'orchestrator_plan_ready' | 'task_completed' | 'task_cancelled' | 'task_failed'; provider_id?: string; attempt?: number; routing_reason?: string; score?: number | null; result?: OrchestratorResult; detail?: string; reason_code?: string; from_provider_id?: string; to_provider_id?: string }
const plannerPreflightCodes = [
  'planner_spawn_failed', 'planner_initialize_failed', 'planner_config_read_failed', 'planner_mcp_config_invalid',
  'planner_thread_start_failed', 'planner_sandbox_rejected', 'planner_approval_policy_rejected', 'planner_cwd_rejected',
  'planner_workspace_roots_rejected', 'planner_instruction_sources_rejected', 'planner_permission_profile_rejected',
  'planner_thread_id_invalid', 'planner_cleanup_failed',
] as const
type PlannerPreflightCode = typeof plannerPreflightCodes[number]
type PlannerPreflightProbe = { ready: boolean; diagnosticCode: PlannerPreflightCode | null }
const plannerErrorCodes = [
  'cancelled', 'unsupported_capability', 'invalid_request', 'unavailable', 'protocol_error', 'backend_failed', 'event_sink_closed',
  ...plannerPreflightCodes,
  'planner_turn_start_failed', 'planner_turn_id_invalid', 'planner_turn_transport_failed', 'planner_turn_timeout',
  'planner_turn_unexpected_notification', 'planner_turn_unexpected_item', 'planner_turn_failed',
  'planner_response_missing', 'planner_plan_invalid',
] as const
const isPlannerErrorCode = (value: unknown): value is typeof plannerErrorCodes[number] => typeof value === 'string' && (plannerErrorCodes as readonly string[]).includes(value)
const isPlannerPreflightCode = (value: unknown): value is PlannerPreflightCode => typeof value === 'string' && (plannerPreflightCodes as readonly string[]).includes(value)
const labels: Record<Role, string> = { conversation: 'Conversa', summary: 'Resumo', orchestrator: 'Orchestrator' }
const numberValue = (value: string) => value === '' ? NaN : Number(value)

function RoleForm({ initial, providers, onSaved }: { initial: Policy; providers: ProviderInfo[]; onSaved: (policy: Policy) => void }) {
  const [policy, setPolicy] = useState(initial)
  const usable = (provider: ProviderInfo | undefined) => Boolean(provider?.enabled && provider.configured && provider.capabilities.textGeneration && provider.capabilities.streaming)
  const targetConfigs = useRef<Record<string, Target>>(Object.fromEntries(initial.targets.map(target => [target.providerId, target])))
  const available = providers.filter(provider => !policy.targets.some(target => target.providerId === provider.id))
  function newTarget(id: string): Target {
    return targetConfigs.current[id] ?? { providerId: id, model: providers.find(provider => provider.id === id)?.defaultModel ?? '', thinkingLevel: null }
  }
  function replaceTarget(index: number, target: Target) {
    setPolicy(current => ({ ...current, targets: current.targets.map((item, i) => i === index ? target : item) }))
  }
  function changeProvider(index: number, id: string) {
    targetConfigs.current[policy.targets[index].providerId] = policy.targets[index]
    replaceTarget(index, newTarget(id))
  }
  function moveTarget(index: number, offset: number) {
    setPolicy(current => {
      const targets = [...current.targets]
      ;[targets[index], targets[index + offset]] = [targets[index + offset], targets[index]]
      return { ...current, targets }
    })
  }
  function changeMode(routingMode: Routing) {
    policy.targets.forEach(target => { targetConfigs.current[target.providerId] = target })
    let targets = policy.targets
    if (routingMode === 'fixed') targets = targets.slice(0, 1)
    else if (targets.length < 2 && available[0]) targets = [...targets, newTarget(available[0].id)]
    setPolicy(current => ({ ...current, routingMode, targets }))
  }
  const [customOutput, setCustomOutput] = useState(String(initial.maxOutputTokens ?? 4096))
  const [calls, setCalls] = useState(String(initial.maxProviderCalls))
  const [retries, setRetries] = useState(String(initial.maxRetries))
  const [backoff, setBackoff] = useState(String(initial.retryBackoffMs))
  const [historyMessages, setHistoryMessages] = useState(String(initial.historyMaxMessages))
  const [historyBytes, setHistoryBytes] = useState(String(initial.historyMaxBytes))
  const [summaryBytes, setSummaryBytes] = useState(String(initial.summaryInputMaxBytes))
  const [contextBytes, setContextBytes] = useState(String(initial.contextMaxBytes))
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState('')
  const [error, setError] = useState('')
  async function save() {
    const maxProviderCalls = numberValue(calls)
    const maxOutputTokens = policy.maxOutputTokens === null ? null : numberValue(customOutput)
    if (policy.targets.length < (policy.routingMode === 'fixed' ? 1 : 2) || policy.targets.length > 8 || (policy.routingMode === 'fixed' && policy.targets.length !== 1) || new Set(policy.targets.map(target => target.providerId)).size !== policy.targets.length) { setError('Quantidade de targets inválida para este modo.'); return }
    for (const target of policy.targets) {
      const provider = providers.find(item => item.id === target.providerId)
      if (!usable(provider)) { setError('Configure uma credencial para cada provider habilitado e compatível antes de salvar.'); return }
      if (!target.model.trim() || target.model !== target.model.trim() || new TextEncoder().encode(target.model).length > 128 || /[\u0000-\u001f\u007f]/.test(target.model)) { setError('Modelo inválido: use até 128 bytes, sem controles.'); return }
      if (target.thinkingLevel && !provider?.supportedThinkingLevels.includes(target.thinkingLevel)) { setError('Thinking indisponível neste target.'); return }
    }
    if (!Number.isSafeInteger(maxProviderCalls) || maxProviderCalls < 1 || maxProviderCalls > 4294967295) { setError('Max provider calls deve estar entre 1 e 4294967295.'); return }
    if (policy.routingMode !== 'fixed' && maxProviderCalls < 2) { setError('Fallback real exige pelo menos 2 provider calls por tarefa.'); return }
    if (maxOutputTokens !== null && (!Number.isSafeInteger(maxOutputTokens) || maxOutputTokens < 1 || maxOutputTokens > 4294967295)) { setError('O limite de output deve estar entre 1 e 4294967295.'); return }
    const maxRetries = numberValue(retries), retryBackoffMs = numberValue(backoff)
    const historyMaxMessages = numberValue(historyMessages), historyMaxBytes = numberValue(historyBytes)
    const summaryInputMaxBytes = numberValue(summaryBytes)
    const contextMaxBytes = numberValue(contextBytes)
    if ([maxRetries, retryBackoffMs, historyMaxMessages, historyMaxBytes, summaryInputMaxBytes, contextMaxBytes].some(value => !Number.isSafeInteger(value) || value < 0 || value > 4294967295) || contextMaxBytes < 1) { setError('Limites avançados devem ser inteiros não negativos até 4294967295.'); return }
    setBusy(true); setError(''); setMessage('')
    try {
      const saved = await invoke<Policy>('update_cognitive_role_policy', { policy: { ...policy, maxOutputTokens, maxProviderCalls, maxRetries, retryBackoffMs, historyMaxMessages, historyMaxBytes, summaryInputMaxBytes, contextMaxBytes } })
      setPolicy(saved); onSaved(saved); setMessage('Salvo. A próxima tarefa usará esta configuração.')
    } catch (cause) { setError(`Não foi possível salvar: ${String(cause)}`) }
    finally { setBusy(false) }
  }
  return <section className="settings-card role-card" aria-label={labels[initial.role]}>
    <div className="role-header"><div><p className="settings-kicker">PAPEL COGNITIVO</p><h2>{labels[initial.role]}</h2></div><span>{initial.role}</span></div>
    <div className="settings-fields">
      <fieldset><legend>Roteamento</legend>
        <label>Modo<select value={policy.routingMode} onChange={event => changeMode(event.target.value as Routing)}><option value="fixed">Fixed</option><option value="preferred" disabled={providers.length < 2}>Preferred</option><option value="auto" disabled={providers.length < 2}>Auto</option></select></label>
        {policy.routingMode === 'fixed' && <small>Usa exatamente o único target definido.</small>}
        {policy.routingMode === 'preferred' && <small>usa a ordem definida acima; falhas elegíveis podem avançar antes do primeiro chunk.</small>}
        {policy.routingMode === 'auto' && <small>a Luna escolhe apenas entre os targets autorizados acima, considerando preferência, disponibilidade e continuidade da sessão.</small>}
        <ol>{policy.targets.map((target, index) => {
          const provider = providers.find(item => item.id === target.providerId)
          return <li key={target.providerId}><fieldset><legend>Target {index + 1}</legend>
            <label>Provider<select value={target.providerId} onChange={event => changeProvider(index, event.target.value)}>{providers.filter(item => item.id === target.providerId || !policy.targets.some(other => other.providerId === item.id)).map(item => <option key={item.id} value={item.id}>{item.displayName}{!usable(item) ? ' · indisponível' : ''}</option>)}</select></label>
            {!usable(provider) && <p className="settings-warning">Provider desabilitado, incompatível ou sem credencial.</p>}
            <label>Modelo <small>Default: {provider?.defaultModel ?? '—'}</small><input value={target.model} maxLength={128} onChange={event => replaceTarget(index, { ...target, model: event.target.value })} /></label>
            <label>Thinking<select value={target.thinkingLevel ?? ''} onChange={event => replaceTarget(index, { ...target, thinkingLevel: (event.target.value || null) as Thinking })}><option value="">Padrão do provider</option>{provider?.supportedThinkingLevels.filter((level): level is Exclude<Thinking, null> => level !== null).map(level => <option key={level} value={level}>{level}</option>)}</select></label>
            <div className="settings-actions"><button type="button" aria-label={`Mover target ${index + 1} para cima`} disabled={index === 0} onClick={() => moveTarget(index, -1)}>↑</button><button type="button" aria-label={`Mover target ${index + 1} para baixo`} disabled={index === policy.targets.length - 1} onClick={() => moveTarget(index, 1)}>↓</button><button type="button" disabled={policy.targets.length <= (policy.routingMode === 'fixed' ? 1 : 2)} onClick={() => { targetConfigs.current[target.providerId] = target; setPolicy({ ...policy, targets: policy.targets.filter((_, i) => i !== index) }) }}>Remover</button></div>
          </fieldset></li>
        })}</ol>
        <label>Adicionar target<select value="" disabled={policy.routingMode === 'fixed' || policy.targets.length >= 8 || available.length === 0} onChange={event => setPolicy({ ...policy, targets: [...policy.targets, newTarget(event.target.value)] })}><option value="">Selecione provider ainda não usado</option>{available.map(provider => <option key={provider.id} value={provider.id}>{provider.displayName}{!usable(provider) ? ' · indisponível' : ''}</option>)}</select></label>
        {numberValue(calls) < policy.targets.length && <p className="settings-warning">Max provider calls pode impedir percorrer todos os targets.</p>}
      </fieldset>
      <fieldset><legend>Output</legend>
        <label className="radio"><input type="radio" checked={policy.maxOutputTokens === null} onChange={() => setPolicy({ ...policy, maxOutputTokens: null })} />Padrão do provider / sem limite adicional da Luna</label>
        <label className="radio"><input type="radio" checked={policy.maxOutputTokens !== null} onChange={() => setPolicy({ ...policy, maxOutputTokens: numberValue(customOutput) || 1 })} />Limite personalizado</label>
        {policy.maxOutputTokens !== null && <input aria-label={`Limite de output para ${labels[initial.role]}`} type="number" min="1" max="4294967295" value={customOutput} onChange={event => setCustomOutput(event.target.value)} />}
        <small>Padrão do provider remove o teto adicional da Luna. Os limites reais do provider/modelo continuam válidos.</small>
      </fieldset>
      <label>Max provider calls <small>Limite total de chamadas que uma tarefa pode realizar, incluindo tentativas adicionais e futuros fallbacks. Chunks de uma resposta streaming não são novas chamadas.</small><input type="number" min="1" max="4294967295" value={calls} onChange={event => setCalls(event.target.value)} /></label>
      <fieldset><legend>Retry</legend>
        <label className="radio"><input type="checkbox" checked={policy.retryEnabled} onChange={event => setPolicy({ ...policy, retryEnabled: event.target.checked })} />Retry automático para falhas transitórias</label>
        <label>Tentativas extras<input type="number" min="0" value={retries} onChange={event => setRetries(event.target.value)} /></label>
        <label>Backoff inicial (ms)<input type="number" min="0" value={backoff} onChange={event => setBackoff(event.target.value)} /></label>
        <small>O retry só ocorre antes do primeiro trecho e para Timeout/Unavailable sem Retry-After. Rate limit e Unavailable com Retry-After entram em cooldown; em Preferred/Auto, podem seguir ao próximo target se ainda houver orçamento.</small>
        {numberValue(retries) + 1 > numberValue(calls) && <p className="settings-warning">Seu orçamento total permite menos tentativas do que o número de retries configurado.</p>}
        {policy.routingMode !== 'fixed' && policy.retryEnabled && numberValue(retries) > 0 && numberValue(calls) < 1 + (policy.targets.length - 1) * (numberValue(retries) + 1) && <p className="settings-warning">Retries podem consumir o orçamento antes de alcançar todos os targets. Aumente Max provider calls para reservar chamadas para fallback.</p>}
      </fieldset>
      {initial.role === 'conversation' && <fieldset><legend>Histórico enviado</legend>
        <label>Máximo de mensagens anteriores<input type="number" min="0" value={historyMessages} onChange={event => setHistoryMessages(event.target.value)} /></label>
        <label>Máximo de bytes<input type="number" min="0" value={historyBytes} onChange={event => setHistoryBytes(event.target.value)} /></label>
        <small>0 desativa o envio de histórico anterior. Apenas a sessão atual é usada.</small>
      </fieldset>}
      {initial.role === 'orchestrator' && <fieldset><legend>Contexto do planejamento</legend>
        <label>Máximo de bytes do contexto<input type="number" min="1" value={contextBytes} onChange={event => setContextBytes(event.target.value)} /></label>
        <small>O planejamento usa somente o objetivo atual e identidade técnica mínima; histórico e memória não são enviados.</small>
      </fieldset>}
      {initial.role === 'summary' && <fieldset><legend>Input do resumo</legend>
        <label>Máximo de bytes<input type="number" min="0" value={summaryBytes} onChange={event => setSummaryBytes(event.target.value)} /></label>
        <small>Limita quanto do transcript fechado pode ser enviado para gerar título e resumo.</small>
      </fieldset>}
    </div>
    <div className="settings-actions"><button type="button" disabled={busy} onClick={() => void save()}>Salvar {labels[initial.role].toLowerCase()}</button>{message && <span role="status">{message}</span>}</div>
    {error && <p className="settings-error" role="alert">{error}</p>}
  </section>
}


function TimeoutForm({ provider, initial, onSaved }: { provider: ProviderInfo; initial: Timeouts; onSaved: (timeouts: Timeouts) => void }) {
  const [request, setRequest] = useState(String(initial.requestTimeoutMs))
  const [idle, setIdle] = useState(String(initial.streamIdleTimeoutMs))
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState('')
  const [error, setError] = useState('')
  async function save() {
    const requestTimeoutMs = numberValue(request), streamIdleTimeoutMs = numberValue(idle)
    if ([requestTimeoutMs, streamIdleTimeoutMs].some(value => !Number.isSafeInteger(value) || value < 1 || value > 4294967295)) {
      setError('Timeouts devem ser inteiros positivos em milissegundos.'); return
    }
    setBusy(true); setError(''); setMessage('')
    try {
      const saved = await invoke<Timeouts>('update_provider_timeouts', { providerId: provider.id, timeouts: { requestTimeoutMs, streamIdleTimeoutMs } })
      onSaved(saved); setMessage(`Salvo. Novas chamadas ${provider.displayName} usarão estes timeouts.`)
    } catch (cause) { setError(`Não foi possível salvar: ${String(cause)}`) }
    finally { setBusy(false) }
  }
  return <fieldset><legend>Timeouts de {provider.displayName}</legend>
    <div className="settings-fields">
      <label>Timeout HTTP total (ms)<input type="number" min="1" value={request} onChange={event => setRequest(event.target.value)} /></label>
      <label>Timeout sem dados no stream (ms)<input type="number" min="1" value={idle} onChange={event => setIdle(event.target.value)} /></label>
      <small>Conexão: 8 s — configuração técnica do adapter. As duas opções acima usam snapshot por tarefa.</small>
    </div>
    <div className="settings-actions"><button type="button" disabled={busy} onClick={() => void save()}>Salvar timeouts</button>{message && <span role="status">{message}</span>}</div>
    {error && <p className="settings-error" role="alert">{error}</p>}
  </fieldset>
}

export default function AiSettingsApp() {
  const [settings, setSettings] = useState<Settings | null>(null)
  const [geminiKey, setGeminiKey] = useState('')
  const [groqKey, setGroqKey] = useState('')
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)
  const [probing, setProbing] = useState(false)
  const [probeOutput, setProbeOutput] = useState('')
  const [probeResult, setProbeResult] = useState<ProbeResult | null>(null)
  const [codex, setCodex] = useState<CodexRuntimeStatus | null>(null)
  const [codexBusy, setCodexBusy] = useState(false)
  const [codexAppServer, setCodexAppServer] = useState<CodexAppServerProbe | null>(null)
  const [codexAppServerBusy, setCodexAppServerBusy] = useState(false)
  const [plannerObjective, setPlannerObjective] = useState('')
  const [plannerResult, setPlannerResult] = useState<PlanV1 | null>(null)
  const [plannerBusy, setPlannerBusy] = useState(false)
  const [plannerError, setPlannerError] = useState('')
  const [plannerPreflight, setPlannerPreflight] = useState<PlannerPreflightProbe | null>(null)
  const [plannerPreflightBusy, setPlannerPreflightBusy] = useState(false)
  const [orchestratorObjective, setOrchestratorObjective] = useState('Organizar uma tarefa simples em passos seguros')
  const [orchestratorResult, setOrchestratorResult] = useState<OrchestratorResult | null>(null)
  const [orchestratorBusy, setOrchestratorBusy] = useState(false)
  const [orchestratorError, setOrchestratorError] = useState('')
  const [orchestratorTaskId, setOrchestratorTaskId] = useState<number | null>(null)
  const [orchestratorRoute, setOrchestratorRoute] = useState('')
  const [orchestratorState, setOrchestratorState] = useState<OrchestratorEvent['state'] | null>(null)
  const orchestratorTaskRef = useRef<number | null>(null)
  const orchestratorTerminalRef = useRef(false)
  async function refresh() { setSettings(await invoke<Settings>('get_ai_settings')) }
  async function refreshCodex() {
    setCodexBusy(true)
    try { setCodex(await invoke<CodexRuntimeStatus>('get_codex_runtime_status')) }
    catch { setError('Não foi possível consultar o runtime Codex.') }
    finally { setCodexBusy(false) }
  }
  async function probeCodexAppServer() {
    setCodexAppServerBusy(true)
    setCodexAppServer(null)
    try { setCodexAppServer(await invoke<CodexAppServerProbe>('probe_codex_app_server')) }
    catch { setCodexAppServer({ launched: false, initialized: false, platformFamily: null, platformOs: null, diagnosticCode: 'codex_app_server_spawn_failed' }) }
    finally { setCodexAppServerBusy(false) }
  }
  async function probeCodexPlanner() {
    setPlannerBusy(true); setPlannerResult(null); setPlannerError('')
    try { setPlannerResult(await invoke<PlanV1>('probe_codex_planner', { objective: plannerObjective })) }
    catch (cause) { setPlannerError(isPlannerErrorCode(cause) ? `Planner falhou: ${cause}` : 'Planner falhou: erro desconhecido') }
    finally { setPlannerBusy(false) }
  }
  async function probeCodexPlannerPreflight() {
    setPlannerPreflightBusy(true); setPlannerPreflight(null)
    try {
      const probe = await invoke<PlannerPreflightProbe>('probe_codex_planner_preflight')
      setPlannerPreflight({ ready: probe.ready === true, diagnosticCode: isPlannerPreflightCode(probe.diagnosticCode) ? probe.diagnosticCode : null })
    } catch { setPlannerPreflight({ ready: false, diagnosticCode: null }) }
    finally { setPlannerPreflightBusy(false) }
  }
  async function runOrchestrator() {
    orchestratorTerminalRef.current = false; setOrchestratorBusy(true); setOrchestratorResult(null); setOrchestratorError(''); setOrchestratorState('pending'); setOrchestratorRoute('')
    try {
      const channel = new Channel<OrchestratorEvent>(event => {
        if (event.type === 'provider_selected') setOrchestratorRoute(`${event.provider_id} · ${event.routing_reason} · score ${event.score ?? '—'} · tentativa ${event.attempt}`)
        setOrchestratorTaskId(event.taskId); setOrchestratorState(event.state)
        if (event.type === 'orchestrator_plan_ready' && event.result) setOrchestratorResult(event.result)
        if (event.type === 'task_failed') setOrchestratorError(`Planejamento falhou: ${event.detail ?? 'erro sanitizado'}`)
        if (event.state === 'completed' || event.state === 'cancelled' || event.state === 'failed') {
          orchestratorTerminalRef.current = true; setOrchestratorBusy(false); orchestratorTaskRef.current = null
        }
      })
      const id = await invoke<number>('start_orchestrator_planning', { objective: orchestratorObjective, channel })
      setOrchestratorTaskId(id)
      if (!orchestratorTerminalRef.current) orchestratorTaskRef.current = id
    }
    catch (cause) { setOrchestratorBusy(false); setOrchestratorError(typeof cause === 'string' ? `Planejamento falhou: ${cause}` : 'Planejamento falhou: erro sanitizado') }
  }
  async function cancelOrchestrator() {
    if (orchestratorTaskRef.current === null) return
    try { await invoke<boolean>('cancel_task', { taskId: orchestratorTaskRef.current }) }
    catch { setOrchestratorError('Não foi possível solicitar o cancelamento.') }
  }
  useEffect(() => {
    void refresh().catch(() => setError('Não foi possível carregar as configurações.'))
    void refreshCodex()
    return () => {
      if (orchestratorTaskRef.current !== null) void invoke<boolean>('cancel_task', { taskId: orchestratorTaskRef.current })
    }
  }, [])
  async function credential(provider: 'gemini' | 'groq', action: 'set' | 'delete') {
    setBusy(true); setError(''); setNotice('')
    const command = `${provider}_${action === 'set' ? 'set_api_key' : 'delete_api_key'}`
    const value = provider === 'gemini' ? geminiKey : groqKey
    try {
      await invoke(command, action === 'set' ? { apiKey: value } : {})
      if (provider === 'gemini') setGeminiKey(''); else setGroqKey('')
      await refresh()
      setNotice(`${provider === 'gemini' ? 'Gemini' : 'Groq'}: ${action === 'set' ? 'chave guardada no SecretStore' : 'chave removida'}.`)
    } catch { setError('Não foi possível alterar a credencial no SecretStore.') }
    finally { setBusy(false) }
  }
  async function probeGroq() {
    setProbing(true); setError(''); setProbeOutput(''); setProbeResult(null)
    try {
      const channel = new Channel<ProbeEvent>(event => {
        if (event.type === 'chunk') setProbeOutput(current => current + event.text)
      })
      const result = await invoke<ProbeResult>('groq_probe', { channel })
      setProbeResult(result)
    } catch (cause) { setError(`Diagnóstico Groq falhou: ${String(cause)}`) }
    finally { setProbing(false) }
  }
  const gemini = settings?.providers.find(provider => provider.id === 'gemini')
  const groq = settings?.providers.find(provider => provider.id === 'groq')
  return <main className="settings-page">
    <header><p className="settings-kicker">LUNA · COGNIÇÃO</p><h1>IA e modelos</h1><p>Estas escolhas são aplicadas na próxima tarefa. Uma resposta em andamento mantém a configuração com que começou.</p></header>
    {settings ? <>
      <section className="settings-card"><h2>Providers disponíveis</h2>
        <p>{settings.providers.map(provider => `${provider.displayName} · ${provider.configured ? 'Configurado' : 'Não configurado'}`).join(' · ')} · Cofre {settings.credentialStoreAvailable ? 'disponível' : 'indisponível'}</p>
        <p>Cada papel permite Fixed, Preferred e Auto sobre targets autorizados.</p>
        <label>Chave API Gemini <input type="password" autoComplete="off" value={geminiKey} onChange={event => setGeminiKey(event.target.value)} placeholder="Definir ou substituir chave Gemini" /></label>
        <div className="settings-actions"><button disabled={busy || !geminiKey.trim()} onClick={() => void credential('gemini', 'set')}>Guardar Gemini</button><button disabled={busy || !gemini?.configured} onClick={() => void credential('gemini', 'delete')}>Remover Gemini</button></div>
        <label>Chave API Groq <input type="password" autoComplete="off" value={groqKey} onChange={event => setGroqKey(event.target.value)} placeholder="Definir ou substituir chave Groq" /></label>
        <div className="settings-actions"><button disabled={busy || !groqKey.trim()} onClick={() => void credential('groq', 'set')}>Guardar Groq</button><button disabled={busy || !groq?.configured} onClick={() => void credential('groq', 'delete')}>Remover Groq</button></div>
        {notice && <p role="status">{notice}</p>}
        {settings.providers.map(provider => settings.providerTimeouts[provider.id] && <TimeoutForm key={provider.id} provider={provider} initial={settings.providerTimeouts[provider.id]} onSaved={saved => setSettings(current => current && ({ ...current, providerTimeouts: { ...current.providerTimeouts, [provider.id]: saved } }))} />)}
      </section>
      <section className="settings-card"><h2>Diagnóstico Groq</h2>
        <p>Executa uma chamada <strong>Fixed(groq)</strong> isolada com <code>openai/gpt-oss-20b</code>. Não altera a policy da conversa nem do Summary.</p>
        <div className="settings-actions"><button disabled={probing || !groq?.configured} onClick={() => void probeGroq()}>{probing ? 'Executando…' : 'Testar Groq'}</button></div>
        {probeOutput && <p aria-live="polite"><strong>Stream:</strong> {probeOutput}</p>}
        {probeResult && <p role="status">Provider: {probeResult.providerId} · input {probeResult.usage.inputTokens} · output {probeResult.usage.outputTokens} · total {probeResult.usage.totalTokens ?? '—'} tokens.</p>}
      </section>
      <section className="settings-card" aria-labelledby="codex-status-title">
        <p className="settings-kicker">EXPERIMENTAL · AGENT RUNTIME</p><h2 id="codex-status-title">Codex</h2>
        <p>O status consulta somente o runtime local e a autenticação reportada pelo CLI. O teste separado do Planner faz uma chamada de modelo para propor um plano; nenhum passo é executado.</p>
        <div className="codex-status-grid">
          <span>Runtime</span><strong>{codex?.installed ? 'disponível' : codex ? 'não encontrado' : 'consultando…'}</strong>
          <span>Versão</span><strong>{codex?.version ?? '—'}</strong>
          <span>Autenticação</span><strong>{codex ? ({ chatgpt: 'ChatGPT', api_key: 'API key', other: 'outro método', unknown: 'desconhecida', none: 'não autenticado' }[codex.authKind]) : '—'}</strong>
          <span>Estado</span><strong>{codex?.available ? 'disponível' : codex ? 'indisponível' : '—'}</strong>
        </div>
        {codex?.diagnosticCode && <small>Diagnóstico: {codex.diagnosticCode}</small>}
        <div className="settings-actions"><button type="button" disabled={codexBusy} onClick={() => void refreshCodex()}>{codexBusy ? 'Atualizando…' : 'Atualizar status'}</button></div>
        <h3>App-server</h3>
        <p>O teste inicia o processo, conclui o handshake e o encerra. Nenhuma chamada de modelo é feita.</p>
        <div className="codex-status-grid">
          <span>Estado</span><strong>{codexAppServer?.initialized ? 'conectado' : codexAppServer ? 'indisponível' : 'não testado'}</strong>
          <span>Plataforma</span><strong>{codexAppServer?.platformOs ? ({ linux: 'Linux', macos: 'macOS', windows: 'Windows' }[codexAppServer.platformOs]) : '—'}</strong>
        </div>
        {codexAppServer?.diagnosticCode && <small>Diagnóstico: {codexAppServer.diagnosticCode}</small>}
        <div className="settings-actions"><button type="button" disabled={codexAppServerBusy} onClick={() => void probeCodexAppServer()}>{codexAppServerBusy ? 'Testando…' : 'Testar app-server'}</button></div>
        <h3>Planner experimental</h3>
        <p>Propõe um plano estruturado para diagnóstico. O plano não é executado.</p>
        <div className="settings-actions"><button type="button" disabled={plannerPreflightBusy || plannerBusy} onClick={() => void probeCodexPlannerPreflight()}>{plannerPreflightBusy ? 'Testando isolamento…' : 'Testar isolamento'}</button></div>
        {plannerPreflight && <p role="status">Isolamento: {plannerPreflight.ready ? 'pronto' : 'bloqueado'}{plannerPreflight.diagnosticCode && <> · Diagnóstico: {plannerPreflight.diagnosticCode}</>}</p>}
        <label>Objetivo <textarea value={plannerObjective} maxLength={2048} onChange={event => setPlannerObjective(event.target.value)} rows={4} /></label>
        <div className="settings-actions"><button type="button" disabled={plannerBusy || plannerPreflightBusy || !plannerObjective.trim()} onClick={() => void probeCodexPlanner()}>{plannerBusy ? 'Aguardando plano…' : 'Testar Planner'}</button></div>
        {plannerError && <p role="alert">{plannerError}</p>}
        {plannerResult && <div role="status" className="planner-result">
          <p><strong>Objetivo:</strong> {plannerResult.objective}</p>
          <ol>{plannerResult.steps.map(step => <li key={step.id}><strong>{step.id}:</strong> {step.description}<br />Dependências: {step.dependsOn.join(', ') || 'nenhuma'}<br />Capabilities: {step.requiredCapabilities.join(', ') || 'nenhuma'}</li>)}</ol>
          <p><strong>Riscos:</strong> {plannerResult.risks.join('; ') || 'nenhum'}</p>
          <p><strong>Perguntas:</strong> {plannerResult.questions.join('; ') || 'nenhuma'}</p>
        </div>}
      </section>
      <section className="settings-card" aria-labelledby="orchestrator-title">
        <p className="settings-kicker">DEV · COGNITIVE ROLE</p><h2 id="orchestrator-title">Orchestrator / Planner</h2>
        <p>Executa uma operação real com a policy persistida do Orchestrator. O Core valida o PlanV1; nenhum passo ou ferramenta é executado.</p>
        <label>Objetivo controlado<textarea value={orchestratorObjective} maxLength={2048} onChange={event => setOrchestratorObjective(event.target.value)} rows={3} /></label>
        <div className="settings-actions"><button type="button" disabled={orchestratorBusy || !orchestratorObjective.trim()} onClick={() => void runOrchestrator()}>{orchestratorBusy ? 'Planejando…' : 'Executar planejamento real'}</button><button type="button" disabled={!orchestratorBusy || orchestratorTaskRef.current === null} onClick={() => void cancelOrchestrator()}>Cancelar</button></div>
        <p role="status">TaskId: {orchestratorTaskId ?? '—'} · Estado: {orchestratorState ?? 'sem tarefa'} · {orchestratorRoute}</p>
        {orchestratorError && <p role="alert" className="settings-error">{orchestratorError}</p>}
        {orchestratorResult && <div role="status" className="planner-result"><p><strong>Provider efetivamente usado:</strong> {orchestratorResult.providerId}</p><p><strong>Calls:</strong> {orchestratorResult.usage.providerCalls} · <strong>Output:</strong> {orchestratorResult.usage.outputTokens} tokens</p><p><strong>Objetivo:</strong> {orchestratorResult.plan.objective}</p><ol>{orchestratorResult.plan.steps.map(step => <li key={step.id}><strong>{step.id}:</strong> {step.description}<br />Dependências: {step.dependsOn.join(', ') || 'nenhuma'}<br />Capabilities: {step.requiredCapabilities.join(', ') || 'nenhuma'}</li>)}</ol><p><strong>Riscos:</strong> {orchestratorResult.plan.risks.join('; ') || 'nenhum'}</p><p><strong>Perguntas:</strong> {orchestratorResult.plan.questions.join('; ') || 'nenhuma'}</p></div>}
      </section>
      <div className="role-grid">{settings.roles.map(role => <RoleForm key={role.role} initial={role} providers={settings.providers} onSaved={saved => setSettings(current => current && ({ ...current, roles: current.roles.map(item => item.role === saved.role ? saved : item) }))} />)}</div>
      <section className="settings-card"><h2>Parâmetros avançados</h2>
        <p>Streaming: ativo — requerido pelo adapter atual. Thinking summaries: desativado — não configurável nesta versão.</p>
        <p>Timeout HTTP total e idle do stream: configuráveis por provider. Conexão: 8 s — configuração dos adapters atuais.</p>
        <p>Histórico de conversa enviado: configurável em Conversa. Summary input budget: configurável em Resumo. Orchestrator input budget: bytes UTF-8 da instrução fixa mais objetivo enviados ao provider. Retry: configurável por papel.</p>
        <p>Preparação de resumo: até 256 mensagens candidatas mais a primeira fala do usuário; leitura local limitada a 8193 caracteres por mensagem. Título gerado: até 70 caracteres; resumo: até 1200. Limites técnicos desta versão.</p>
        <p>Fixed usa um target; Preferred segue a ordem; Auto usa score determinístico e continuidade de sessão na Conversa. Task graph permanece adiado.</p>
        <p>Cada target usa seu próprio modelo, thinking e timeouts. Temperature, top-p e tools ainda não são expostos.</p>
        <p>Segurança, isolamento de sessão e segredos fora do React são invariantes do aplicativo. O adapter Gemini envia <code>store:false</code>; o adapter Groq não envia parâmetros não suportados pelo endpoint Chat Completions.</p>
      </section>
    </> : <p>Carregando…</p>}
    {error && <p className="settings-error" role="alert">{error}</p>}
  </main>
}
