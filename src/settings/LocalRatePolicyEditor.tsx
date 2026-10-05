import { useState } from 'react'
import { invoke } from '@tauri-apps/api/core'
import type { ProviderRate, RatePolicy } from './providerRate'
import { quotaLabels } from './providerOperational'
import { editedPolicy, policyDraft } from './ratePolicyDraft'
import type { LimitDraft, PolicyDraft } from './ratePolicyDraft'

const saveErrors: Record<string, string> = {
  rate_policy_invalid: 'Policy inválida segundo o runtime.', rate_policy_busy: 'Existem operações pendentes. Aguarde e salve novamente.',
  rate_state_unavailable: 'Estado local de rate indisponível; a gravação não foi confirmada.', provider_unavailable: 'Provider indisponível.', worker_failed: 'Não foi possível concluir a gravação.',
}
export function LocalRatePolicyEditor({ rate, refresh }: { rate: ProviderRate; refresh: () => Promise<boolean> }) {
  const [edit, setEdit] = useState<{ draft: PolicyDraft; base: RatePolicy; limitsEdited: boolean; budgetEdited: boolean } | null>(null)
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState('')
  const [error, setError] = useState('')
  const draft = edit?.draft ?? policyDraft(rate.policy)
  function change(section: 'limits' | 'budget', update: (draft: PolicyDraft) => PolicyDraft) {
    setEdit(previous => {
      const state = previous ?? { draft: policyDraft(rate.policy), base: rate.policy, limitsEdited: false, budgetEdited: false }
      return { ...state, draft: update(state.draft), limitsEdited: state.limitsEdited || section === 'limits', budgetEdited: state.budgetEdited || section === 'budget' }
    })
    setNotice(''); setError('')
  }
  function row(index: number, value: Partial<LimitDraft>) {
    change('limits', d => ({ ...d, limits: d.limits.map((l, i) => i === index ? { ...l, ...value } : l) }))
  }
  async function save() {
    if (!edit || rate.persistenceFailed || busy) return
    let policy: RatePolicy
    try { policy = editedPolicy(edit.draft, edit.base, rate.policy, edit.limitsEdited, edit.budgetEdited) }
    catch (cause) { setError(cause instanceof Error ? cause.message : 'Revise os campos da policy.'); return }
    setBusy(true); setError(''); setNotice('')
    try {
      await invoke('update_provider_rate_policy', { providerId: rate.providerId, policy })
      // Keep the submitted form visible while the post-write snapshot is pending.
      const refreshed = await refresh()
      if (refreshed) setEdit(null)
      setNotice(refreshed ? 'Limites locais salvos e snapshot operacional atualizado.' : 'Limites locais salvos. O refresh não concluiu; sua edição permanece visível. Aguarde uma captura válida e recarregue o formulário.')
    } catch (cause) {
      setError(typeof cause === 'string' && Object.hasOwn(saveErrors, cause) ? saveErrors[cause] : 'Não foi possível salvar a policy local.')
    } finally { setBusy(false) }
  }
  return <details className="operations-details"><summary>Limites locais da Luna</summary>
    <p>Configuração local explícita; o backend valida e preserva o accounting conforme LR-8C. Alterar ou remover a semântica de uma janela muda a policy; alterar somente a capacidade preserva consumo na mesma janela.</p>
    <p>Anchors são Unix ms UTC. Nenhuma conversão de timezone. Budgets de tokens não garantem bloqueio pré-HTTP sem um teto total comprovado pelo adapter; accounting incompleto permanece desconhecido.</p>
    <form onSubmit={event => { event.preventDefault(); void save() }}>
      <fieldset disabled={busy || rate.persistenceFailed}><legend>limits[]</legend>
        {draft.limits.length === 0 && <p>Nenhum limite local configurado.</p>}
        {draft.limits.map((l, index) => <fieldset key={index}><legend>Limite {index + 1}</legend>
          <div className="operations-form-grid">
            <label>Scope<select value={l.scope} onChange={e => row(index, { scope: e.target.value as LimitDraft['scope'] })}><option value="provider">Provider</option><option value="model">Model</option></select></label>
            {l.scope === 'model' && <label>Modelo<input value={l.model} onChange={e => row(index, { model: e.target.value })} /></label>}
            <label>Dimensão<select value={l.dimension} onChange={e => row(index, { dimension: e.target.value as LimitDraft['dimension'] })}><option value="">Selecione</option>{(['requests_per_minute', 'tokens_per_minute', 'requests_per_day', 'tokens_per_day'] as const).map(d => <option key={d} value={d}>{quotaLabels[d]} · {d}</option>)}</select></label>
            <label>Capacidade<input type="number" min="0" step="1" value={l.capacity} onChange={e => row(index, { capacity: e.target.value })} /></label>
            <label>window.periodMs<input type="number" min="1" step="1" value={l.period} onChange={e => row(index, { period: e.target.value })} /></label>
            <label>window.anchorUnixMs (UTC)<input type="number" min="0" step="1" value={l.anchor} onChange={e => row(index, { anchor: e.target.value })} /></label>
          </div>
          <button type="button" onClick={() => change('limits', d => ({ ...d, limits: d.limits.filter((_, i) => i !== index) }))}>Remover limite {index + 1}</button>
        </fieldset>)}
        <button type="button" disabled={draft.limits.length >= 64} onClick={() => change('limits', d => ({ ...d, limits: [...d.limits, { scope: 'provider', model: '', dimension: '', capacity: '', period: '', anchor: '' }] }))}>Adicionar limite local</button>
      </fieldset>
      <fieldset disabled={busy || rate.persistenceFailed}><legend>DailyBudget</legend>
        <label className="radio"><input type="checkbox" checked={draft.budgetEnabled} onChange={e => change('budget', d => ({ ...d, budgetEnabled: e.target.checked }))} />Habilitar budget diário local</label>
        {draft.budgetEnabled && <div className="operations-form-grid">
          <label>anchorUnixMs (UTC)<input type="number" min="0" step="1" value={draft.anchor} onChange={e => change('budget', d => ({ ...d, anchor: e.target.value }))} /></label>
          <label>maxRequests (vazio = não configurado)<input type="number" min="0" step="1" value={draft.requests} onChange={e => change('budget', d => ({ ...d, requests: e.target.value }))} /></label>
          <label>maxAccountedTokens (vazio = não configurado)<input type="number" min="0" step="1" value={draft.tokens} onChange={e => change('budget', d => ({ ...d, tokens: e.target.value }))} /></label>
          <p>Período: 86.400.000 ms. Ambos os máximos vazios deixam as duas dimensões sem budget configurado.</p>
        </div>}
      </fieldset>
      <div className="settings-actions"><button disabled={!edit || busy || rate.persistenceFailed} type="submit">{busy ? 'Salvando…' : 'Salvar limites locais'}</button><button type="button" disabled={busy} onClick={() => { setEdit(null); setError(''); setNotice('Formulário recarregado do snapshot atual.') }}>Recarregar formulário</button></div>
    </form>
    {notice && <p role="status">{notice}</p>}{error && <p role="alert" className="settings-error">{error}</p>}
  </details>
}
