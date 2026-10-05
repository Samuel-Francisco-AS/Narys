import type { ProviderAdmission } from './providerAdmission'
import type { ProviderRate } from './providerRate'
import type { CircuitTransitionReason, ProviderResilience } from './providerResilience'
import type { Fact, QuotaDimension, QuotaScope, Timing, UsageDimension } from './providerTelemetry'
import type { ProviderTelemetry } from './providerTelemetry'

export type OperationalProviderInfo = { id: string; displayName: string; configured: boolean; enabled: boolean }
export type ProviderOperationalSnapshot = {
  capturedAtUnixMs: number | null
  telemetry: ProviderTelemetry[]
  admission: ProviderAdmission[]
  rate: ProviderRate[]
  resilience: ProviderResilience[]
}
export const unknown = 'Desconhecido'
export const numberText = (value: number | null | undefined) => value == null ? unknown : value.toLocaleString('pt-BR')
export const durationText = (value: number | null | undefined) => value == null ? unknown : `${numberText(value)} ms`
export const unixText = (value: number | null | undefined) => value == null ? unknown : `${numberText(value)} (Unix ms UTC)`
export function capturePresentation(value: number | null | undefined): { label: string; title?: string } {
  if (value == null || !Number.isSafeInteger(value) || value < 0 || value > 8_640_000_000_000_000) {
    return { label: `Última captura: ${unknown}` }
  }
  return { label: 'Última captura recebida', title: `${new Date(value).toISOString()} · ${unixText(value)}` }
}
export const timingText = (value: Timing) => value.kind === 'delay_ms' ? `${durationText(value.value)} após observação` : unixText(value.value)
export function factText<T>(fact: Fact<T>, format: (value: T) => string): string {
  return fact.state === 'known' ? format(fact.value) : unknown
}
export const provenanceLabels = {
  provider_header: 'Provider', provider_response: 'Resposta do provider',
  user_configuration: 'Configuração local', local_runtime: 'Runtime Luna',
} satisfies Record<Extract<Fact<number>, { state: 'known' }>['provenance'], string>
export function factOrigin<T>(fact: Fact<T>): string {
  return fact.state === 'known' ? `${provenanceLabels[fact.provenance]} · observado em ${unixText(fact.observedAtUnixMs)}` : unknown
}
export const scopeText = (scope: QuotaScope) => scope.kind === 'provider' ? 'Provider' : `Model (${scope.model})`
export const quotaLabels: Record<QuotaDimension, string> = {
  requests_per_minute: 'RPM', tokens_per_minute: 'TPM', requests_per_day: 'RPD', tokens_per_day: 'TPD', concurrency: 'Concurrency',
}
export const usageLabels: Record<UsageDimension, string> = {
  requests: 'Requests observadas', input_tokens: 'Input tokens', output_tokens: 'Output tokens', total_tokens: 'Total tokens', thought_tokens: 'Thought tokens',
}
export const transitionLabels: Record<CircuitTransitionReason, string> = {
  failure_threshold_timeout: 'Threshold de timeouts atingido',
  failure_threshold_unavailable: 'Threshold de indisponibilidade atingido',
  open_duration_elapsed: 'Janela Open concluída', probe_timeout: 'Probe Half-open terminou em timeout',
  probe_unavailable: 'Probe Half-open encontrou indisponibilidade', probe_succeeded: 'Probe Half-open recuperou o provider',
  credential_context_changed: 'Contexto de credencial alterado',
}
const outcomeLabels: Record<string, string> = {
  rate_limited: 'rate_limited', timeout: 'timeout', quota_exceeded: 'quota_exceeded', provider_auth_failed: 'provider_auth_failed',
  fatal: 'fatal', cancelled: 'cancelled', provider_cancelled: 'provider_cancelled', provider_incomplete: 'provider_incomplete',
  provider_requires_action: 'provider_requires_action', provider_protocol_error: 'provider_protocol_error',
  model_or_request_rejected: 'model_or_request_rejected', unavailable: 'unavailable', channel_closed: 'channel_closed',
  provider_mode_unsupported: 'provider_mode_unsupported', provider_output_limit_exceeded: 'provider_output_limit_exceeded',
}
export function outcomeText(value: { kind: 'succeeded' } | { kind: 'failed'; code: string }): string {
  return value.kind === 'succeeded' ? 'sucesso' : Object.hasOwn(outcomeLabels, value.code) ? outcomeLabels[value.code] : 'Código não reconhecido'
}
/** Presentation only: no reservation, route or remote-success prediction. Model
 * gates remain explicitly scoped; token uncertainty cannot establish a request block. */
export function operationalConditions(provider: OperationalProviderInfo, admission?: ProviderAdmission, rate?: ProviderRate, resilience?: ProviderResilience): string[] {
  const conditions: string[] = []
  if (!provider.configured) conditions.push('Não configurado')
  if (!provider.enabled) conditions.push('Desabilitado')
  if (!admission || !rate || !resilience) conditions.push('Estado operacional desconhecido')
  if (resilience?.circuitState === 'open') conditions.push(resilience.openRemainingMs > 0 ? 'Circuito aberto' : 'Circuito Open · janela concluída; próxima autorização pode sondar')
  if (resilience?.circuitState === 'half_open') conditions.push(`Half-open / sondagem${resilience.halfOpenProbesActive >= resilience.halfOpenMaxProbes ? ' · probes ocupados' : ''}`)
  if (resilience && resilience.cooldownRemainingMs > 0) conditions.push('Cooldown')
  if (rate?.persistenceFailed) conditions.push('Bloqueado por estado local conhecido · falha de persistência')
  if (rate && rate.contextGeneration >= Number.MAX_SAFE_INTEGER) conditions.push('Bloqueado por estado local conhecido · geração esgotada')
  for (const c of rate?.constraints ?? []) {
    if ((c.dimension === 'requests_per_minute' || c.dimension === 'requests_per_day') && c.effectiveRemaining !== null && c.effectiveRemaining < 1) {
      conditions.push(`${c.scope.kind === 'provider' ? 'Bloqueado por constraint conhecida' : 'Restrição conhecida somente neste modelo'} · ${scopeText(c.scope)} · ${quotaLabels[c.dimension]} · ${c.source}`)
    }
  }
  if (admission) {
    if (admission.activeCalls >= admission.maxConcurrency) conditions.push('Concurrency ocupada · tentativa pode aguardar na fila')
    if ((admission.activeCalls >= admission.maxConcurrency || admission.queueDepth > 0) && admission.queueDepth >= admission.queueCapacity) conditions.push('Fila local sem vaga conhecida')
  }
  if (conditions.length === 0) conditions.push('Disponível para tentativa')
  else if (conditions.every(text => text.startsWith('Restrição conhecida somente neste modelo'))) conditions.unshift('Disponível para tentativa · depende do modelo')
  return conditions
}
