import { isTauri } from '@tauri-apps/api/core'
import { getCurrentWindow, PhysicalSize, type PhysicalPosition } from '@tauri-apps/api/window'

export type WindowErgonomicsState = {
  available: boolean
  alwaysOnTop: boolean
  clickThrough: boolean
  recovery: 'unavailable' | 'ready'
  position: { x: number; y: number } | null
  busy: boolean
  error: string | null
}

export const initialWindowErgonomicsState: WindowErgonomicsState = {
  available: false,
  alwaysOnTop: false,
  clickThrough: false,
  recovery: 'unavailable',
  position: null,
  busy: false,
  error: null,
}

/** Owns desktop window operations. No avatar or render policy lives here. */
export class WindowController {
  private readonly window = isTauri() ? getCurrentWindow() : null
  private state = initialWindowErgonomicsState
  private unlistenMoved: (() => void) | null = null
  private disposed = false
  private layout: 'presence' | 'composer' | 'conversation' | null = null
  private layoutQueue: Promise<boolean> = Promise.resolve(true)

  constructor(private readonly onChange: (state: WindowErgonomicsState) => void) {}

  get canDrag(): boolean {
    return this.window !== null && !this.disposed
  }

  async initialize(): Promise<void> {
    if (!this.window) return
    // A WebView reload retains the native window size. Avoid redundant resize
    // when the Tauri config already opened Presence at the intended size.
    if (Math.round(window.innerWidth * window.devicePixelRatio) === 310
      && Math.round(window.innerHeight * window.devicePixelRatio) === 410) this.layout = 'presence'
    else await this.setLayout('presence')
    try {
      const alwaysOnTop = await this.window.isAlwaysOnTop()
      const position = await this.window.outerPosition()
      if (this.disposed) return
      this.publish({ available: true, alwaysOnTop, position: this.coordinates(position) })

      const unlisten = await this.window.onMoved(({ payload }) => {
        this.publish({ position: this.coordinates(payload) })
      })
      if (this.disposed) unlisten()
      else this.unlistenMoved = unlisten
    } catch (error) {
      this.publish({ error: this.message(error) })
    }
  }

  applyAlwaysOnTopPreference(enabled: boolean): void {
    this.publish({ alwaysOnTop: enabled })
  }

  async startDragging(): Promise<void> {
    if (!this.window || this.disposed) return
    try {
      await this.window.startDragging()
      this.publish({ error: null })
    } catch (error) {
      this.publish({ error: `Movimentação: ${this.message(error)}` })
    }
  }

  setLayout(layout: 'presence' | 'composer' | 'conversation'): Promise<boolean> {
    const next = this.layoutQueue.then(async () => {
      if (this.layout === layout) return true
      if (!this.window) { this.layout = layout; return true }
      const dimensions = { presence: [310, 410], composer: [310, 490], conversation: [625, 490] } as const
      const [width, height] = dimensions[layout]
      try {
        if (import.meta.env.DEV) console.debug(`[UIP-4-FIX] setSize request ${layout} ${width}×${height}`)
        await this.window.setSize(new PhysicalSize(width, height))
        if (import.meta.env.DEV) console.debug(`[UIP-4-FIX] setSize resolved ${layout}`)
        this.layout = layout
        this.publish({ error: null })
        return true
      } catch (error) {
        this.publish({ error: `Tamanho da janela: ${this.message(error)}` })
        return false
      }
    })
    this.layoutQueue = next
    return next
  }

  /** Reserved for a future externally verified recovery path. */
  async setClickThrough(enabled: boolean): Promise<void> {
    if (!this.window) return
    if (enabled && this.state.recovery !== 'ready') {
      throw new Error('Recuperação externa não comprovada; click-through bloqueado.')
    }
    await this.window.setIgnoreCursorEvents(enabled)
    this.publish({ clickThrough: enabled })
  }

  dispose(): void {
    this.disposed = true
    this.unlistenMoved?.()
    this.unlistenMoved = null
  }

  private coordinates(position: PhysicalPosition): { x: number; y: number } {
    return { x: position.x, y: position.y }
  }

  private message(error: unknown): string {
    return error instanceof Error ? error.message : String(error)
  }

  private publish(update: Partial<WindowErgonomicsState>): void {
    if (this.disposed) return
    this.state = { ...this.state, ...update }
    this.onChange(this.state)
  }
}
