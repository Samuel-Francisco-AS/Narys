/** Transient read-only runtime health. Local defaults are not provider quotas. */
export type CircuitState = 'closed' | 'open' | 'half_open'
export type CircuitTransitionReason =
  | 'failure_threshold_timeout' | 'failure_threshold_unavailable'
  | 'open_duration_elapsed' | 'probe_timeout' | 'probe_unavailable'
  | 'probe_succeeded' | 'credential_context_changed'
export type ProviderResilience = {
  providerId: string
  circuitState: CircuitState
  consecutiveEligibleFailures: number
  configuredThreshold: number
  openRemainingMs: number
  halfOpenProbesActive: number
  halfOpenMaxProbes: number
  cooldownRemainingMs: number
  transitionCount: number
  lastTransitionReason: CircuitTransitionReason | null
  breakerOpenCount: number
  halfOpenCount: number
  recoveryCount: number
  saturated: boolean
}
