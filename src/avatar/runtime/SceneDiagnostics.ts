import type * as THREE from 'three'
import type { RenderBudgetState } from './RenderBudget'

const REPORT_INTERVAL_MS = 5000

function stats(samples: number[]) {
  if (samples.length === 0) return null
  const sorted = [...samples].sort((a, b) => a - b)
  const percentile = (fraction: number) => sorted[Math.ceil(fraction * sorted.length) - 1]
  const mean = samples.reduce((sum, value) => sum + value, 0) / samples.length
  const round = (value: number) => Math.round(value * 100) / 100
  return {
    meanMs: round(mean),
    p50Ms: round(percentile(0.5)),
    p95Ms: round(percentile(0.95)),
    minMs: round(sorted[0]),
    maxMs: round(sorted[sorted.length - 1]),
  }
}

/** Development-only sampling; never drives React state or the render loop. */
export class SceneDiagnostics {
  private readonly frameIntervals: number[] = []
  private readonly frameWork: number[] = []
  private lastFrameAt: number | null = null
  private periodStartedAt = performance.now()
  private frameCount = 0
  private callbackCount = 0
  private throttledCount = 0
  private suspendedCount = 0
  private resizeCount = 0
  private readonly timer: number
  private disposed = false
  private periodRenderMode: RenderBudgetState['mode']

  constructor(
    private readonly container: HTMLElement,
    private readonly renderer: THREE.WebGLRenderer,
    private readonly webglVersion: string,
    private readonly webglRenderer: string,
    private readonly getRenderBudget: () => RenderBudgetState,
  ) {
    this.periodRenderMode = getRenderBudget().mode
    // Complete the fallible initial report before acquiring the timer.
    this.report('start')
    this.timer = window.setInterval(() => this.report('interval'), REPORT_INTERVAL_MS)
  }

  recordFrame(startedAt: number, endedAt: number): void {
    if (this.lastFrameAt !== null) this.frameIntervals.push(startedAt - this.lastFrameAt)
    this.lastFrameAt = startedAt
    this.frameWork.push(endedAt - startedAt)
    this.frameCount++
  }

  recordCallback(skipped: boolean, suspended: boolean): void {
    this.callbackCount++
    if (skipped) {
      if (suspended) this.suspendedCount++
      else this.throttledCount++
    }
  }

  reportTransition(): void {
    this.report('render-mode-change')
    // A mode boundary is not a frame interval, especially across suspension.
    this.lastFrameAt = null
  }

  recordResize(): void {
    this.resizeCount++
  }

  private report(reason: string): void {
    if (this.disposed) return
    const now = performance.now()
    const elapsedMs = now - this.periodStartedAt
    const bounds = this.container.getBoundingClientRect()
    const budget = this.getRenderBudget()
    console.info('[UIP-0]', JSON.stringify({
      at: new Date().toISOString(),
      reason,
      elapsedMs: Math.round(elapsedMs),
      frames: this.frameCount,
      callbacks: this.callbackCount,
      throttledCallbacks: this.throttledCount,
      suspendedCallbacks: this.suspendedCount,
      periodRenderMode: this.periodRenderMode,
      renderMode: budget.mode,
      renderReason: budget.reason,
      targetFps: budget.targetFps,
      fps: elapsedMs > 0 ? Math.round(this.frameCount * 100000 / elapsedMs) / 100 : 0,
      interval: stats(this.frameIntervals),
      updateAndRender: stats(this.frameWork),
      resizeCount: this.resizeCount,
      containerCss: { width: bounds.width, height: bounds.height },
      canvasCss: { width: this.renderer.domElement.clientWidth, height: this.renderer.domElement.clientHeight },
      drawingBuffer: { width: this.renderer.domElement.width, height: this.renderer.domElement.height },
      devicePixelRatio: window.devicePixelRatio,
      rendererPixelRatio: this.renderer.getPixelRatio(),
      visibilityState: document.visibilityState,
      hasFocus: document.hasFocus(),
      webglVersion: this.webglVersion,
      webglRenderer: this.webglRenderer,
    }))
    this.periodStartedAt = now
    this.periodRenderMode = budget.mode
    this.frameCount = 0
    this.callbackCount = 0
    this.throttledCount = 0
    this.suspendedCount = 0
    this.resizeCount = 0
    this.frameIntervals.length = 0
    this.frameWork.length = 0
  }

  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    window.clearInterval(this.timer)
  }
}
