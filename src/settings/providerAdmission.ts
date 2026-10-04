/** Read-only local Luna capacity. Independent of remote quota facts. */
export type TrafficClass = 'foreground_interactive' | 'foreground_task' | 'background'
export type ProviderAdmission = {
  providerId: string
  maxConcurrency: number
  activeCalls: number
  queueDepth: number
  queueCapacity: number
  queuedByClass: Record<TrafficClass, number>
  totalAdmissions: number
  totalWaited: number
  queueDelayTotalMs: number
  queueDelaySamples: number
  queueDelayRecentMs: number | null
  queueFullCount: number
  queueTimeoutCount: number
  countersSaturated: boolean
}
