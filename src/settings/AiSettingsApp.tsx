import { useEffect, useState } from 'react'
import { Channel, invoke } from '@tauri-apps/api/core'
import './settings.css'

type Thinking = 'low' | 'medium' | 'high' | null
type Role = 'conversation' | 'summary'
type Routing = 'fixed' | 'preferred'
type Policy = { role: Role; providerId: string; model: string; thinkingLevel: Thinking; routingMode: Routing; fallbackProviderId: string | null; fallbackModel: string | null; fallbackThinkingLevel: Thinking; maxOutputTokens: number | null; maxProviderCalls: number; retryEnabled: boolean; maxRetries: number; retryBackoffMs: number; historyMaxMessages: number; historyMaxBytes: number; summaryInputMaxBytes: number }
type Timeouts = { requestTimeoutMs: number; streamIdleTimeoutMs: number }
type ProviderInfo = { id: string; displayName: string; configured: boolean; supportedThinkingLevels: string[] }
type Settings = { providerTimeouts: Timeouts; providers: ProviderInfo[]; roles: Policy[]; credentialStoreAvailable: boolean }
type ProbeEvent = { type: 'selected'; providerId: string; attempt: number } | { type: 'chunk'; text: string }
type ProbeResult = { text: string; providerId: string; usage: { providerCalls: number; inputTokens: number; outputTokens: number; totalTokens: number | null; thoughtTokens: number | null; retries: number; fallbacks: number } }
const labels: Record<Role, string> = { conversation: 'Conversa', summary: 'Resumo' }
const numberValue = (value: string) => value === '' ? NaN : Number(value)

function RoleForm({ initial, groqConfigured, onSaved }: { initial: Policy; groqConfigured: boolean; onSaved: (policy: Policy) => void }) {
  const [policy, setPolicy] = useState(initial)
  const [customOutput, setCustomOutput] = useState(String(initial.maxOutputTokens ?? 4096))
  const [calls, setCalls] = useState(String(initial.maxProviderCalls))
  const [retries, setRetries] = useState(String(initial.maxRetries))
  const [backoff, setBackoff] = useState(String(initial.retryBackoffMs))
  const [historyMessages, setHistoryMessages] = useState(String(initial.historyMaxMessages))
  const [historyBytes, setHistoryBytes] = useState(String(initial.historyMaxBytes))
  const [summaryBytes, setSummaryBytes] = useState(String(initial.summaryInputMaxBytes))
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState('')
  const [error, setError] = useState('')
  async function save() {
    const maxProviderCalls = numberValue(calls)
    const maxOutputTokens = policy.maxOutputTokens === null ? null : numberValue(customOutput)
    if (!policy.model.trim() || policy.model !== policy.model.trim() || policy.model.length > 128 || /[\u0000-\u001f\u007f]/.test(policy.model)) { setError('Modelo inválido. Use um identificador sem controles, até 128 caracteres.'); return }
    if (policy.routingMode === 'preferred') {
      const fallbackModel = policy.fallbackModel ?? ''
      if (initial.role !== 'conversation' || policy.fallbackProviderId !== 'groq' || !fallbackModel.trim() || fallbackModel !== fallbackModel.trim() || fallbackModel.length > 128 || /[\u0000-\u001f\u007f]/.test(fallbackModel)) { setError('Fallback inválido. A LR-7C usa Groq com um identificador de modelo válido.'); return }
      if (!groqConfigured) { setError('Configure a chave Groq antes de ativar o fallback real.'); return }
    }
    if (!Number.isSafeInteger(maxProviderCalls) || maxProviderCalls < 1 || maxProviderCalls > 4294967295) { setError('Max provider calls deve estar entre 1 e 4294967295.'); return }
    if (policy.routingMode === 'preferred' && maxProviderCalls < 2) { setError('Fallback real exige pelo menos 2 provider calls por tarefa.'); return }
    if (maxOutputTokens !== null && (!Number.isSafeInteger(maxOutputTokens) || maxOutputTokens < 1 || maxOutputTokens > 4294967295)) { setError('O limite de output deve estar entre 1 e 4294967295.'); return }
    const maxRetries = numberValue(retries), retryBackoffMs = numberValue(backoff)
    const historyMaxMessages = numberValue(historyMessages), historyMaxBytes = numberValue(historyBytes)
    const summaryInputMaxBytes = numberValue(summaryBytes)
    if ([maxRetries, retryBackoffMs, historyMaxMessages, historyMaxBytes, summaryInputMaxBytes].some(value => !Number.isSafeInteger(value) || value < 0 || value > 4294967295)) { setError('Limites avançados devem ser inteiros não negativos até 4294967295.'); return }
    setBusy(true); setError(''); setMessage('')
    try {
      const saved = await invoke<Policy>('update_cognitive_role_policy', { policy: { ...policy, maxOutputTokens, maxProviderCalls, maxRetries, retryBackoffMs, historyMaxMessages, historyMaxBytes, summaryInputMaxBytes } })
      setPolicy(saved); onSaved(saved); setMessage('Salvo. A próxima tarefa usará esta configuração.')
    } catch (cause) { setError(`Não foi possível salvar: ${String(cause)}`) }
    finally { setBusy(false) }
  }
  return <section className="settings-card role-card" aria-label={labels[initial.role]}>
    <div className="role-header"><div><p className="settings-kicker">PAPEL COGNITIVO</p><h2>{labels[initial.role]}</h2></div><span>{initial.role}</span></div>
    <div className="settings-fields">
      <label>Provider<select value={policy.providerId} onChange={event => setPolicy({ ...policy, providerId: event.target.value })}><option value="gemini">Gemini</option></select></label>
      <label>Modelo <small>Identificador do provider · default atual: gemini-3.8-flash</small><input value={policy.model} maxLength={128} onChange={event => setPolicy({ ...policy, model: event.target.value })} /></label>
      <label>Thinking<select value={policy.thinkingLevel ?? ''} onChange={event => setPolicy({ ...policy, thinkingLevel: (event.target.value || null) as Thinking })}>
        <option value="">Padrão do provider</option><option value="low">Low</option><option value="medium">Medium</option><option value="high">High</option>
      </select></label>
      {initial.role === 'conversation' && <fieldset><legend>Roteamento</legend>
        <label>Modo<select value={policy.routingMode} onChange={event => setPolicy({ ...policy, routingMode: event.target.value as Routing })}>
          <option value="fixed">Fixed · somente Gemini</option>
          <option value="preferred">Preferred · Gemini → Groq quando elegível</option>
        </select></label>
        {policy.routingMode === 'preferred' && <>
          <label>Fallback<select value={policy.fallbackProviderId ?? 'groq'} onChange={() => setPolicy({ ...policy, fallbackProviderId: 'groq' })}><option value="groq">Groq</option></select></label>
          <label>Modelo do fallback<input value={policy.fallbackModel ?? ''} maxLength={128} onChange={event => setPolicy({ ...policy, fallbackProviderId: 'groq', fallbackModel: event.target.value })} /></label>
          <label>Thinking do fallback<select value={policy.fallbackThinkingLevel ?? ''} onChange={event => setPolicy({ ...policy, fallbackThinkingLevel: (event.target.value || null) as Thinking })}>
            <option value="">Padrão do provider</option><option value="low">Low</option><option value="medium">Medium</option><option value="high">High</option>
          </select></label>
          <small>Fallback só acontece antes do primeiro chunk e apenas para erros elegíveis/cooldown. Autenticação, quota terminal, request inválido e falha após chunk não trocam de provider.</small>
          {!groqConfigured && <p className="settings-warning">Groq não está configurado. Salve a chave antes de ativar Preferred.</p>}
        </>}
      </fieldset>}
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
        <small>O retry só ocorre antes do primeiro trecho e para Timeout/Unavailable sem Retry-After. Rate limit e Unavailable com Retry-After entram em cooldown; em Preferred, podem seguir ao fallback se ainda houver orçamento.</small>
        {numberValue(retries) + 1 > numberValue(calls) && <p className="settings-warning">Seu orçamento total permite menos tentativas do que o número de retries configurado.</p>}
        {policy.routingMode === 'preferred' && policy.retryEnabled && numberValue(retries) > 0 && numberValue(calls) <= numberValue(retries) + 1 && <p className="settings-warning">Um timeout pode consumir todo o orçamento em retries do Gemini antes de chegar ao Groq. Aumente Max provider calls se quiser reservar uma chamada para fallback.</p>}
      </fieldset>
      {initial.role === 'conversation' && <fieldset><legend>Histórico enviado</legend>
        <label>Máximo de mensagens anteriores<input type="number" min="0" value={historyMessages} onChange={event => setHistoryMessages(event.target.value)} /></label>
        <label>Máximo de bytes<input type="number" min="0" value={historyBytes} onChange={event => setHistoryBytes(event.target.value)} /></label>
        <small>0 desativa o envio de histórico anterior. Apenas a sessão atual é usada.</small>
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


function TimeoutForm({ initial, onSaved }: { initial: Timeouts; onSaved: (timeouts: Timeouts) => void }) {
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
      const saved = await invoke<Timeouts>('update_gemini_timeouts', { timeouts: { requestTimeoutMs, streamIdleTimeoutMs } })
      onSaved(saved); setMessage('Salvo. Novas chamadas Gemini usarão estes timeouts.')
    } catch (cause) { setError(`Não foi possível salvar: ${String(cause)}`) }
    finally { setBusy(false) }
  }
  return <fieldset><legend>Timeouts globais do Gemini</legend>
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
  async function refresh() { setSettings(await invoke<Settings>('get_ai_settings')) }
  useEffect(() => { void refresh().catch(() => setError('Não foi possível carregar as configurações.')) }, [])
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
        <p>Gemini · {gemini?.configured ? 'Configurado' : 'Não configurado'} · Groq · {groq?.configured ? 'Configurado' : 'Não configurado'} · Cofre {settings.credentialStoreAvailable ? 'disponível' : 'indisponível'}</p>
        <p>LR-7C: a Conversa pode usar Gemini como preferido e Groq como fallback explícito. Resumo continua Fixed(Gemini) até um gate próprio.</p>
        <label>Chave API Gemini <input type="password" autoComplete="off" value={geminiKey} onChange={event => setGeminiKey(event.target.value)} placeholder="Definir ou substituir chave Gemini" /></label>
        <div className="settings-actions"><button disabled={busy || !geminiKey.trim()} onClick={() => void credential('gemini', 'set')}>Guardar Gemini</button><button disabled={busy || !gemini?.configured} onClick={() => void credential('gemini', 'delete')}>Remover Gemini</button></div>
        <label>Chave API Groq <input type="password" autoComplete="off" value={groqKey} onChange={event => setGroqKey(event.target.value)} placeholder="Definir ou substituir chave Groq" /></label>
        <div className="settings-actions"><button disabled={busy || !groqKey.trim()} onClick={() => void credential('groq', 'set')}>Guardar Groq</button><button disabled={busy || !groq?.configured} onClick={() => void credential('groq', 'delete')}>Remover Groq</button></div>
        {notice && <p role="status">{notice}</p>}
        <TimeoutForm initial={settings.providerTimeouts} onSaved={saved => setSettings(current => current && ({ ...current, providerTimeouts: saved }))} />
      </section>
      <section className="settings-card"><h2>Diagnóstico Groq</h2>
        <p>Executa uma chamada <strong>Fixed(groq)</strong> isolada com <code>openai/gpt-oss-20b</code>. Não altera a policy da conversa nem do Summary.</p>
        <div className="settings-actions"><button disabled={probing || !groq?.configured} onClick={() => void probeGroq()}>{probing ? 'Executando…' : 'Testar Groq'}</button></div>
        {probeOutput && <p aria-live="polite"><strong>Stream:</strong> {probeOutput}</p>}
        {probeResult && <p role="status">Provider: {probeResult.providerId} · input {probeResult.usage.inputTokens} · output {probeResult.usage.outputTokens} · total {probeResult.usage.totalTokens ?? '—'} tokens.</p>}
      </section>
      <div className="role-grid">{settings.roles.map(role => <RoleForm key={role.role} initial={role} groqConfigured={Boolean(groq?.configured)} onSaved={saved => setSettings(current => current && ({ ...current, roles: current.roles.map(item => item.role === saved.role ? saved : item) }))} />)}</div>
      <section className="settings-card"><h2>Parâmetros avançados</h2>
        <p>Streaming: ativo — requerido pelo adapter atual. Thinking summaries: desativado — não configurável nesta versão.</p>
        <p>Timeout HTTP total e idle do stream: configuráveis globalmente para Gemini. Conexão: 8 s — configuração do adapter atual.</p>
        <p>Histórico de conversa enviado: configurável em Conversa. Summary input budget: configurável em Resumo. Retry: configurável por papel.</p>
        <p>Preparação de resumo: até 256 mensagens candidatas mais a primeira fala do usuário; leitura local limitada a 8193 caracteres por mensagem. Título gerado: até 70 caracteres; resumo: até 1200. Limites técnicos desta versão.</p>
        <p>Fallback: Conversa expõe Fixed ou Preferred(Gemini) com target Groq configurado separadamente. Resumo continua Fixed(Gemini). Auto, affinity e task graph permanecem fora da LR-7C.</p>
        <p>Groq usa modelo/thinking/timeouts próprios; nenhum parâmetro Gemini é reaproveitado silenciosamente. Temperature, top-p e tools ainda não são expostos.</p>
        <p>Segurança, isolamento de sessão e segredos fora do React são invariantes do aplicativo. O adapter Gemini envia <code>store:false</code>; o adapter Groq não envia parâmetros não suportados pelo endpoint Chat Completions.</p>
      </section>
    </> : <p>Carregando…</p>}
    {error && <p className="settings-error" role="alert">{error}</p>}
  </main>
}
