import { isTauri } from '@tauri-apps/api/core'
import { getCurrentWindow, LogicalSize, PhysicalSize, type PhysicalPosition } from '@tauri-apps/api/window'

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
  private presentation: 'economy' | 'presence' | null = null
  private economySize: [number, number] = [1120, 720]
  private layout: 'presence' | 'composer' | 'conversation' | null = null
  private layoutQueue: Promise<boolean> = Promise.resolve(true)

  constructor(private readonly onChange: (state: WindowErgonomicsState) => void) {}

  get canDrag(): boolean {
    return this.window !== null && !this.disposed
  }

  async initialize(): Promise<void> {
    if (!this.window) return
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
      if (this.disposed || this.presentation !== 'presence') return true
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

  setPresentation(mode: 'economy' | 'presence', presenceLayout: 'presence' | 'composer' | 'conversation' = 'presence'): Promise<boolean> {
    const next = this.layoutQueue.then(async () => {
      if (this.disposed) return false
      if (this.presentation === mode) return true
      const previous = this.presentation
      if (mode === 'presence') this.economySize = [Math.max(640, window.innerWidth), Math.max(480, window.innerHeight)]
      this.presentation = mode
      this.layout = null
      if (!this.window) return true
      try {
        if (mode === 'economy') {
          await this.window.setMinSize(new LogicalSize(640, 480))
          await this.window.setResizable(true)
          await this.window.setBackgroundColor('#11151e')
          if (previous === 'presence' || window.innerWidth < 640 || window.innerHeight < 480) await this.window.setSize(new LogicalSize(this.economySize[0], this.economySize[1]))
        } else {
          await this.window.setMinSize(null)
          await this.window.setResizable(false)
          await this.window.setBackgroundColor('#00000000')
          const dimensions = { presence: [310, 410], composer: [310, 490], conversation: [625, 490] } as const
          await this.window.setSize(new PhysicalSize(dimensions[presenceLayout][0], dimensions[presenceLayout][1]))
          this.layout = presenceLayout
        }
        this.publish({ error: null })
        return true
      } catch (error) {
        this.presentation = null
        this.publish({ error: `Presentation da janela: ${this.message(error)}` })
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
