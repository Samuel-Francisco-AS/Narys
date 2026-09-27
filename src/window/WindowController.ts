import { isTauri } from '@tauri-apps/api/core'
import { getCurrentWindow, type PhysicalPosition } from '@tauri-apps/api/window'

const ALWAYS_ON_TOP_KEY = 'luna.window.alwaysOnTop'

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

      const saved = window.localStorage.getItem(ALWAYS_ON_TOP_KEY)
      if (saved === 'true' && !alwaysOnTop) await this.setAlwaysOnTop(true)

      const unlisten = await this.window.onMoved(({ payload }) => {
        this.publish({ position: this.coordinates(payload) })
      })
      if (this.disposed) unlisten()
      else this.unlistenMoved = unlisten
    } catch (error) {
      this.publish({ error: this.message(error) })
    }
  }

  async toggleAlwaysOnTop(): Promise<void> {
    if (!this.window || this.state.busy) return
    await this.setAlwaysOnTop(!this.state.alwaysOnTop)
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

  private async setAlwaysOnTop(enabled: boolean): Promise<void> {
    if (!this.window) return
    this.publish({ busy: true, error: null })
    try {
      await this.window.setAlwaysOnTop(enabled)
      // GTK may report the previous state immediately after the request.
      // Track the requested mode; the compositor decides whether it can honor it.
      this.publish({ alwaysOnTop: enabled })
      window.localStorage.setItem(ALWAYS_ON_TOP_KEY, String(enabled))
    } catch (error) {
      this.publish({ error: `Always-on-top: ${this.message(error)}` })
    } finally {
      this.publish({ busy: false })
    }
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
