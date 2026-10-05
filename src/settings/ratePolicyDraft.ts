import type { RatePolicy } from './providerRate'
export type LimitDraft = { scope: 'provider' | 'model'; model: string; dimension: RatePolicy['limits'][number]['dimension'] | ''; capacity: string; period: string; anchor: string }
export type PolicyDraft = { limits: LimitDraft[]; budgetEnabled: boolean; anchor: string; requests: string; tokens: string }
export function policyDraft(policy: RatePolicy): PolicyDraft {
  return {
    limits: policy.limits.map(l => ({ scope: l.scope.kind, model: l.scope.kind === 'model' ? l.scope.model : '', dimension: l.dimension, capacity: String(l.capacity), period: String(l.window.periodMs), anchor: String(l.window.anchorUnixMs) })),
    budgetEnabled: policy.dailyBudget !== null, anchor: policy.dailyBudget ? String(policy.dailyBudget.anchorUnixMs) : '',
    requests: policy.dailyBudget?.maxRequests == null ? '' : String(policy.dailyBudget.maxRequests),
    tokens: policy.dailyBudget?.maxAccountedTokens == null ? '' : String(policy.dailyBudget.maxAccountedTokens),
  }
}
function integer(text: string): number {
  const value = text.trim() === '' ? NaN : Number(text)
  if (!Number.isSafeInteger(value) || value < 0) throw new Error('Use inteiros seguros não negativos; preencha os campos obrigatórios.')
  return value
}
/** Merge only edited sections with the CURRENT snapshot. Never clear accounting.
 * A conflicting policy change in an edited section requires explicit reload. */
export function editedPolicy(draft: PolicyDraft, base: RatePolicy, current: RatePolicy, limitsEdited: boolean, budgetEdited: boolean): RatePolicy {
  if ((limitsEdited && JSON.stringify(base.limits) !== JSON.stringify(current.limits)) || (budgetEdited && JSON.stringify(base.dailyBudget) !== JSON.stringify(current.dailyBudget))) {
    throw new Error('A policy mudou durante a edição. Recarregue o formulário antes de salvar.')
  }
  let limits = current.limits
  if (limitsEdited) {
    if (draft.limits.length > 64) throw new Error('Use no máximo 64 limites locais.')
    limits = draft.limits.map(l => {
      if (!l.dimension) throw new Error('Selecione a dimensão de cada limite.')
      const periodMs = integer(l.period)
      if (periodMs === 0) throw new Error('O período da janela deve ser positivo.')
      if (l.scope === 'model' && (!l.model || l.model !== l.model.trim() || new TextEncoder().encode(l.model).length > 128 || /[\u0000-\u001f\u007f-\u009f]/.test(l.model))) throw new Error('Modelo inválido: até 128 bytes, sem controles ou espaços nas bordas.')
      return { scope: l.scope === 'provider' ? { kind: 'provider' as const } : { kind: 'model' as const, model: l.model }, dimension: l.dimension, capacity: integer(l.capacity), window: { periodMs, anchorUnixMs: integer(l.anchor) } }
    })
    const keys = limits.map(l => JSON.stringify([l.scope, l.dimension]))
    if (new Set(keys).size !== keys.length) throw new Error('Cada combinação de scope e dimensão deve ser única.')
  }
  const dailyBudget = budgetEdited ? draft.budgetEnabled ? {
    anchorUnixMs: integer(draft.anchor), maxRequests: draft.requests.trim() === '' ? null : integer(draft.requests),
    maxAccountedTokens: draft.tokens.trim() === '' ? null : integer(draft.tokens),
  } : null : current.dailyBudget
  return { limits, dailyBudget }
}
