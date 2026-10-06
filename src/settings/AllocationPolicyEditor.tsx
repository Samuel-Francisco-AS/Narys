import type { AllocationDraft, AllocationProfile, PaidUseMode, VariantSelectionMode } from './allocationPolicyDraft'

export function AllocationPolicyEditor({ draft, disabled, onChange }: { draft: AllocationDraft; disabled: boolean; onChange: (draft: AllocationDraft) => void }) {
  const set = <K extends keyof AllocationDraft>(key: K, value: AllocationDraft[K]) => onChange({ ...draft, [key]: value })
  return <fieldset disabled={disabled} className="allocation-policy"><legend>Auto econômico</legend>
    <small>As regras econômicas só usam fatos conhecidos. A Luna não presume que um recurso desconhecido seja gratuito, pago, rápido ou abundante.</small>
    <label>Perfil<select value={draft.profile} onChange={e => set('profile', e.target.value as AllocationProfile)}>
      <option value="economy">Economia</option><option value="balanced">Balanceado</option><option value="fast">Rápido</option>
    </select></label>
    <small>Economia: preserva recursos/custo. Balanceado: meio-termo. Rápido: valoriza continuidade/latência. Sem fatos conhecidos, não há promessa de economia monetária.</small>
    <label>Seleção de variante<select value={draft.variantMode} onChange={e => set('variantMode', e.target.value as VariantSelectionMode)}>
      <option value="explicit">Somente variante configurada</option><option value="auto">Auto entre variantes conhecidas</option>
    </select></label>
    <small>Este eixo é separado do roteamento entre providers. Auto entre variantes conhecidas só usa modelos/efforts explicitamente descritos e suportados. Não faz discovery remoto.</small>
    <label>Mínimo cognitivo (0–255)<input type="number" min="0" max="255" placeholder="Sem mínimo" value={draft.floor} onChange={e => set('floor', e.target.value)} /></label>
    <small>Vazio: sem mínimo. A escala é local/normalizada e exige evidência explícita.</small>
    <p className="settings-warning">Muitos tiers do catálogo atual são desconhecidos. Configurar um mínimo pode deixar Auto sem candidatos.</p>
    <label className="radio"><input type="checkbox" checked={draft.reserveEnabled} onChange={e => set('reserveEnabled', e.target.checked)} />Habilitar reserva local</label>
    {draft.reserveEnabled && <>
      <label>Reduced abaixo de (%)<input type="number" min="0" max="100" value={draft.reduced} onChange={e => set('reduced', e.target.value)} /></label>
      <label>Reserve abaixo de (%)<input type="number" min="0" max="100" value={draft.reserve} onChange={e => set('reserve', e.target.value)} /></label>
      <small>Thresholds locais: 0 ≤ Reserve ≤ Reduced ≤ 100. Não representam limites comerciais.</small>
    </>}
    <label>Uso pago<select value={draft.paid} onChange={e => set('paid', e.target.value as PaidUseMode)}>
      <option value="deny">Não usar automaticamente recursos com cobrança comprovada</option>
      <option value="allow_known_cost_within_budget">Permitir custo conhecido até um teto</option>
    </select></label>
    {draft.paid === 'allow_known_cost_within_budget' && <>
      <label>Moeda (3 letras ASCII maiúsculas)<input value={draft.currency} maxLength={3} onChange={e => set('currency', e.target.value)} /></label>
      <label>Teto por decisão/invocação (até 6 casas decimais)<input inputMode="decimal" value={draft.budget} onChange={e => set('budget', e.target.value)} /></label>
      <small>Teto autorizado por decisão/invocação. Não é saldo, depósito, orçamento mensal, preço do plano ou débito acumulado. Sem conversão entre moedas.</small>
    </>}
    {disabled && <small>Inativo em Fixed/Preferred. Os valores são preservados ao voltar para Auto.</small>}
  </fieldset>
}
