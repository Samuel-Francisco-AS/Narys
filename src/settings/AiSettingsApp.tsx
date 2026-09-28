import { useEffect, useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import './settings.css'

type Thinking = 'low' | 'medium' | 'high' | null
type Role = 'conversation' | 'summary'
type Policy = { role: Role; providerId: string; model: string; thinkingLevel: Thinking; maxOutputTokens: number | null; maxProviderCalls: number }
type Settings = { providers: { id: string; displayName: string; configured: boolean; supportedThinkingLevels: string[] }[]; roles: Policy[]; credentialStoreAvailable: boolean }
const labels: Record<Role, string> = { conversation: 'Conversa', summary: 'Resumo' }
const numberValue = (value: string) => value === '' ? NaN : Number(value)

function RoleForm({ initial, onSaved }: { initial: Policy; onSaved: (policy: Policy) => void }) {
  const [policy, setPolicy] = useState(initial)
  const [customOutput, setCustomOutput] = useState(String(initial.maxOutputTokens ?? 4096))
  const [calls, setCalls] = useState(String(initial.maxProviderCalls))
  const [busy, setBusy] = useState(false)
  const [message, setMessage] = useState('')
  const [error, setError] = useState('')
  async function save() {
    const maxProviderCalls = numberValue(calls)
    const maxOutputTokens = policy.maxOutputTokens === null ? null : numberValue(customOutput)
    if (!policy.model.trim() || policy.model !== policy.model.trim() || policy.model.length > 128 || /[\u0000-\u001f\u007f]/.test(policy.model)) { setError('Modelo inválido. Use um identificador sem controles, até 128 caracteres.'); return }
    if (!Number.isSafeInteger(maxProviderCalls) || maxProviderCalls < 1 || maxProviderCalls > 4294967295) { setError('Max provider calls deve estar entre 1 e 4294967295.'); return }
    if (maxOutputTokens !== null && (!Number.isSafeInteger(maxOutputTokens) || maxOutputTokens < 1 || maxOutputTokens > 4294967295)) { setError('O limite de output deve estar entre 1 e 4294967295.'); return }
    setBusy(true); setError(''); setMessage('')
    try {
      const saved = await invoke<Policy>('update_cognitive_role_policy', { policy: { ...policy, maxOutputTokens, maxProviderCalls } })
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
      <fieldset><legend>Output</legend>
        <label className="radio"><input type="radio" checked={policy.maxOutputTokens === null} onChange={() => setPolicy({ ...policy, maxOutputTokens: null })} />Padrão do provider / sem limite adicional da Luna</label>
        <label className="radio"><input type="radio" checked={policy.maxOutputTokens !== null} onChange={() => setPolicy({ ...policy, maxOutputTokens: numberValue(customOutput) || 1 })} />Limite personalizado</label>
        {policy.maxOutputTokens !== null && <input aria-label={`Limite de output para ${labels[initial.role]}`} type="number" min="1" max="4294967295" value={customOutput} onChange={event => setCustomOutput(event.target.value)} />}
        <small>Padrão do provider remove o teto adicional da Luna. Os limites reais do provider/modelo continuam válidos.</small>
      </fieldset>
      <label>Max provider calls <small>Inteiro positivo; máximo técnico: 4294967295.</small><input type="number" min="1" max="4294967295" value={calls} onChange={event => setCalls(event.target.value)} /></label>
    </div>
    <div className="settings-actions"><button type="button" disabled={busy} onClick={() => void save()}>Salvar {labels[initial.role].toLowerCase()}</button>{message && <span role="status">{message}</span>}</div>
    {error && <p className="settings-error" role="alert">{error}</p>}
  </section>
}

export default function AiSettingsApp() {
  const [settings, setSettings] = useState<Settings | null>(null)
  const [key, setKey] = useState('')
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)
  async function refresh() { setSettings(await invoke<Settings>('get_ai_settings')) }
  useEffect(() => { void refresh().catch(() => setError('Não foi possível carregar as configurações.')) }, [])
  async function credential(command: 'gemini_set_api_key' | 'gemini_delete_api_key') {
    setBusy(true); setError(''); setNotice('')
    try {
      await invoke(command, command === 'gemini_set_api_key' ? { apiKey: key } : {})
      setKey(''); await refresh(); setNotice(command === 'gemini_set_api_key' ? 'Chave guardada no SecretStore.' : 'Chave removida.')
    } catch { setError('Não foi possível alterar a credencial no SecretStore.') }
    finally { setBusy(false) }
  }
  return <main className="settings-page">
    <header><p className="settings-kicker">LUNA · COGNIÇÃO</p><h1>IA e modelos</h1><p>Estas escolhas são aplicadas na próxima tarefa. Uma resposta em andamento mantém a configuração com que começou.</p></header>
    {settings ? <>
      <section className="settings-card"><h2>Provider disponível</h2><p>Gemini · {settings.providers[0]?.configured ? 'Configurado' : 'Não configurado'} · Cofre {settings.credentialStoreAvailable ? 'disponível' : 'indisponível'}</p>
        <p>Outros providers aparecerão quando a integração estiver disponível.</p>
        <label>Chave API Gemini <input type="password" autoComplete="off" value={key} onChange={event => setKey(event.target.value)} placeholder="Definir ou substituir chave" /></label>
        <div className="settings-actions"><button disabled={busy || !key.trim()} onClick={() => void credential('gemini_set_api_key')}>Guardar chave</button><button disabled={busy || !settings.providers[0]?.configured} onClick={() => void credential('gemini_delete_api_key')}>Remover chave</button></div>
        {notice && <p role="status">{notice}</p>}
      </section>
      <div className="role-grid">{settings.roles.map(role => <RoleForm key={role.role} initial={role} onSaved={saved => setSettings(current => current && ({ ...current, roles: current.roles.map(item => item.role === saved.role ? saved : item) }))} />)}</div>
      <section className="settings-card"><h2>Contrato do runtime</h2><p>Streaming: ativo — requerido pelo adapter atual. Resumos de raciocínio: desativados — ainda não expostos pelo runtime.</p><p>Segurança, isolamento de sessão, segredos fora do React, permissões e <code>store:false</code> são invariantes do aplicativo.</p></section>
    </> : <p>Carregando…</p>}
    {error && <p className="settings-error" role="alert">{error}</p>}
  </main>
}
