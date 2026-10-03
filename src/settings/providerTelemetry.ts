/** Read-only LR-8A facts returned by get_ai_settings; no admission policy. */
export type Fact<T> = { state: 'unknown' } | {
  state: 'known'; value: T; provenance: 'provider_header' | 'provider_response' | 'user_configuration' | 'local_runtime'; observedAtUnixMs: number | null
}
export type Timing = { kind: 'delay_ms' | 'unix_ms'; value: number }
export type UsageDimension = 'requests' | 'input_tokens' | 'output_tokens' | 'total_tokens' | 'thought_tokens'
export type QuotaDimension = 'requests_per_minute' | 'tokens_per_minute' | 'requests_per_day' | 'tokens_per_day' | 'concurrency'
export type ProviderTelemetry = {
  providerId: string
  capturedAtUnixMs: number | null
  updatedAgeMs: number | null
  usage: Record<UsageDimension, { observed: Fact<number>; reportingRequests: number; saturated: boolean }>
  quotas: Record<QuotaDimension, { limit: Fact<number>; remaining: Fact<number>; reset: Fact<Timing> }>
  retryHint: Fact<Timing>
  lastOutcome: Fact<{ kind: 'succeeded' } | { kind: 'failed'; code: string }>
}
