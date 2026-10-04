import type { Fact, QuotaDimension, QuotaScope, Timing } from './providerTelemetry'

/** LR-8C read-only contract. No operational panel is introduced here. */
export type FixedWindow = { periodMs: number; anchorUnixMs: number }
export type RatePolicy = {
  limits: { scope: QuotaScope; dimension: Exclude<QuotaDimension, 'concurrency'>; capacity: number; window: FixedWindow }[]
  dailyBudget: { anchorUnixMs: number; maxRequests: number | null; maxAccountedTokens: number | null } | null
}
export type ProviderRate = {
  providerId: string
  capturedAtUnixMs: number | null
  contextGeneration: number
  policy: RatePolicy
  constraints: {
    scope: QuotaScope; dimension: QuotaDimension
    source: 'external_fact' | 'local_policy' | 'daily_budget'
    provenance: 'provider_header' | 'provider_response' | 'user_configuration' | 'local_runtime' | null
    external: { limit: Fact<number>; remaining: Fact<number>; reset: Fact<Timing> } | null
    capacity: number | null; consumed: number; reserved: number; effectiveRemaining: number | null
    resetUnixMs: number | null; resetInMs: number | null
    saturated: boolean; unaccountedTokenCalls: number
  }[]
  pendingReservations: number
  localBlocks: number
  saturated: boolean
  persistenceFailed: boolean
}
