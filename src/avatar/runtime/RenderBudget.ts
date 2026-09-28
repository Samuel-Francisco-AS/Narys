export type RenderMode = 'active' | 'background' | 'suspended'
export type RenderReason = 'focused' | 'blurred' | 'hidden'

export type RenderBudgetState = {
  mode: RenderMode
  reason: RenderReason
  targetFps: number
}

export type RenderBudgetConfig = { activeFps: number; backgroundFps: number }
export const defaultRenderBudgetConfig: RenderBudgetConfig = { activeFps: 30, backgroundFps: 24 }
// WebKitGTK can deliver only ~2 callbacks/s for a visible, unfocused window.
// Hidden time is rebased separately; this cap only guards unexpected visible stalls.
const MAX_DELTA_SECONDS = 0.6
const SCHEDULER_TOLERANCE_MS = 1

/** Owns frame cadence and animation time for the Avatar Runtime. */
export class RenderBudget {
  private state: RenderBudgetState
  private readonly onTransition: (state: RenderBudgetState) => void
  private config: RenderBudgetConfig
  private nextFrameAt = 0
  private lastProcessedAt: number | null = null

  constructor(onTransition: (state: RenderBudgetState) => void, config: RenderBudgetConfig = defaultRenderBudgetConfig) {
    this.onTransition = onTransition
    this.config = config
    this.state = this.readState()
    document.addEventListener('visibilitychange', this.onEnvironmentChange)
    window.addEventListener('focus', this.onEnvironmentChange)
    window.addEventListener('blur', this.onEnvironmentChange)
  }

  get current(): RenderBudgetState {
    return this.state
  }

  updateConfig(config: RenderBudgetConfig): void {
    this.config = config
    this.state = this.readState()
    this.resetClock()
    this.onTransition(this.state)
  }

  /** null means no animation update or render is due for this callback. */
  sample(now: number): number | null {
    if (this.state.mode === 'suspended') return null
    if (now + SCHEDULER_TOLERANCE_MS < this.nextFrameAt) return null

    const interval = 1000 / this.state.targetFps
    // Keep the average cadence, but never catch up with a burst after a stall.
    this.nextFrameAt = now - this.nextFrameAt > interval
      ? now + interval
      : this.nextFrameAt + interval
    const delta = this.lastProcessedAt === null
      ? 0
      : Math.min((now - this.lastProcessedAt) / 1000, MAX_DELTA_SECONDS)
    this.lastProcessedAt = now
    return delta
  }

  resetClock(): void {
    this.nextFrameAt = 0
    this.lastProcessedAt = null
  }

  dispose(): void {
    document.removeEventListener('visibilitychange', this.onEnvironmentChange)
    window.removeEventListener('focus', this.onEnvironmentChange)
    window.removeEventListener('blur', this.onEnvironmentChange)
  }

  private readState(): RenderBudgetState {
    if (document.visibilityState === 'hidden') {
      return { mode: 'suspended', reason: 'hidden', targetFps: 0 }
    }
    if (!document.hasFocus()) {
      return { mode: 'background', reason: 'blurred', targetFps: this.config.backgroundFps }
    }
    return { mode: 'active', reason: 'focused', targetFps: this.config.activeFps }
  }

  private readonly onEnvironmentChange = () => {
    const next = this.readState()
    if (next.mode === this.state.mode && next.reason === this.state.reason) return
    const wasSuspended = this.state.mode === 'suspended'
    this.state = next
    this.nextFrameAt = 0
    if (wasSuspended || next.mode === 'suspended') this.lastProcessedAt = null
    this.onTransition(next)
  }
}
