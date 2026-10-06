export type CognitiveRole = 'conversation' | 'summary' | 'orchestrator' | 'worker'
export type AllocationProfile = 'economy' | 'balanced' | 'fast'
export type VariantSelectionMode = 'explicit' | 'auto'
export type PaidUseMode = 'deny' | 'allow_known_cost_within_budget'
export type RoleAllocationPolicy = {
  role: CognitiveRole
  allocationProfile: AllocationProfile
  variantSelectionMode: VariantSelectionMode
  minimumCognitiveTier: number | null
  paidUsePolicy: PaidUseMode
  maxPaidCurrency: string | null
  maxPaidMicros: number | null
  reducedBelowPercent: number | null
  reserveBelowPercent: number | null
}
export type AllocationDraft = {
  profile: AllocationProfile
  variantMode: VariantSelectionMode
  floor: string
  paid: PaidUseMode
  currency: string
  budget: string
  reserveEnabled: boolean
  reduced: string
  reserve: string
}
const maxMicros = 9007199254740991n
// Fixed point: no float multiplication or rounding can increase authorization.
export function parseBudgetMicros(value: string): number {
  if (!/^\d{1,16}(?:\.\d{1,6})?$/.test(value)) throw new Error('Teto inválido: use decimal não negativo com até 6 casas.')
  const [whole, fraction = ''] = value.split('.')
  const micros = BigInt(whole) * 1000000n + BigInt(fraction.padEnd(6, '0'))
  if (micros > maxMicros) throw new Error('Teto acima do limite de precisão permitido.')
  return Number(micros)
}
export function formatBudgetMicros(value: number): string {
  if (!Number.isSafeInteger(value) || value < 0) throw new Error('Teto inválido.')
  const micros = BigInt(value)
  return `${micros / 1000000n}.${String(micros % 1000000n).padStart(6, '0')}`
}
export function allocationDraft(value: RoleAllocationPolicy): AllocationDraft {
  return {
    profile: value.allocationProfile, variantMode: value.variantSelectionMode,
    floor: value.minimumCognitiveTier === null ? '' : String(value.minimumCognitiveTier),
    paid: value.paidUsePolicy, currency: value.maxPaidCurrency ?? '',
    budget: value.maxPaidMicros === null ? '' : formatBudgetMicros(value.maxPaidMicros),
    reserveEnabled: value.reducedBelowPercent !== null,
    reduced: value.reducedBelowPercent === null ? '' : String(value.reducedBelowPercent),
    reserve: value.reserveBelowPercent === null ? '' : String(value.reserveBelowPercent),
  }
}
function integer(value: string, max: number, label: string): number {
  if (!/^\d{1,3}$/.test(value) || Number(value) > max) throw new Error(`${label}: use inteiro entre 0 e ${max}.`)
  return Number(value)
}
export function validateAllocationDraft(role: CognitiveRole, draft: AllocationDraft): RoleAllocationPolicy {
  if (!['economy', 'balanced', 'fast'].includes(draft.profile) || !['explicit', 'auto'].includes(draft.variantMode) || !['deny', 'allow_known_cost_within_budget'].includes(draft.paid)) throw new Error('Policy econômica inválida.')
  const allow = draft.paid === 'allow_known_cost_within_budget'
  if (allow && !/^[A-Z]{3}$/.test(draft.currency)) throw new Error('Moeda: use exatamente 3 letras ASCII maiúsculas.')
  const reduced = draft.reserveEnabled ? integer(draft.reduced, 100, 'Reduced') : null
  const reserve = draft.reserveEnabled ? integer(draft.reserve, 100, 'Reserve') : null
  if (reserve !== null && reduced !== null && reserve > reduced) throw new Error('Reserve deve ser menor ou igual a Reduced.')
  return {
    role, allocationProfile: draft.profile, variantSelectionMode: draft.variantMode,
    minimumCognitiveTier: draft.floor === '' ? null : integer(draft.floor, 255, 'Mínimo cognitivo'),
    paidUsePolicy: draft.paid, maxPaidCurrency: allow ? draft.currency : null,
    maxPaidMicros: allow ? parseBudgetMicros(draft.budget) : null,
    reducedBelowPercent: reduced, reserveBelowPercent: reserve,
  }
}
