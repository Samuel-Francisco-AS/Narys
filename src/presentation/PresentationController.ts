export type PresentationMode = 'economy' | 'presence' | 'headless'
export type PresentationPhase = 'detached' | 'loading' | 'ready' | 'error'
export type PresentationState = {
  mode: PresentationMode
  phase: PresentationPhase
  generation: number
}

/** Surface selection only. Never starts/cancels tasks or owns a native window. */
export class PresentationController {
  private state: PresentationState
  private readonly listeners = new Set<() => void>()

  constructor(mode: PresentationMode = 'economy') {
    this.state = { mode, phase: mode === 'presence' ? 'loading' : 'detached', generation: 1 }
  }

  readonly getSnapshot = (): PresentationState => this.state
  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    return () => { this.listeners.delete(listener) }
  }

  setMode(mode: PresentationMode): void {
    if (mode !== 'presence' && mode !== 'economy' && mode !== 'headless') throw new Error('presentation_mode_invalid')
    if (mode === this.state.mode) return
    this.publish({ mode, phase: mode === 'presence' ? 'loading' : 'detached', generation: this.state.generation + 1 })
  }

  recreatePresence(): void {
    if (this.state.mode !== 'presence') return
    this.publish({ ...this.state, phase: 'loading', generation: this.state.generation + 1 })
  }

  report(generation: number, phase: 'ready' | 'error'): void {
    if (this.state.mode !== 'presence' || generation !== this.state.generation || phase === this.state.phase) return
    this.publish({ ...this.state, phase })
  }

  private publish(state: PresentationState): void {
    this.state = state
    this.listeners.forEach(listener => listener())
  }
}
