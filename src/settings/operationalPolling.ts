/** One timeout and at most one IPC in flight, including React StrictMode restart.
 * Only the last snapshot is owned by the caller. Hidden/disposed responses are
 * ignored; manual refresh during IPC coalesces one fresh read after it finishes. */
export class OperationalPoller<T> {
  private active = false
  private visible = false
  private epoch = 0
  private pending: Promise<boolean> | null = null
  private queued = false
  private timer: ReturnType<typeof setTimeout> | undefined
  constructor(private readonly read: () => Promise<T>, private readonly receive: (value: T) => void,
    private readonly failed: () => void, private readonly busy: (value: boolean) => void,
    private readonly intervalMs = 1000) {}
  start(visible: boolean) { this.active = true; this.setVisible(visible) }
  stop() { this.active = false; this.visible = false; this.epoch++; this.queued = false; this.clearTimer() }
  setVisible(visible: boolean) {
    this.visible = visible; this.epoch++; this.clearTimer()
    if (this.active && visible) void this.refresh()
  }
  private clearTimer() { clearTimeout(this.timer); this.timer = undefined }
  refresh(): Promise<boolean> {
    if (!this.active || !this.visible) return Promise.resolve(false)
    this.clearTimer()
    if (this.pending) { this.queued = true; return this.pending }
    this.busy(true)
    this.pending = this.drain().finally(() => {
      this.pending = null
      if (this.active) this.busy(false)
      if (this.active && this.visible) this.timer = setTimeout(() => { void this.refresh() }, this.intervalMs)
    })
    return this.pending
  }
  private async drain(): Promise<boolean> {
    let received = false
    do {
      received = false
      this.queued = false
      const epoch = this.epoch
      try {
        const value = await this.read()
        if (this.active && this.visible && this.epoch === epoch) { this.receive(value); received = true }
      } catch {
        if (this.active && this.visible && this.epoch === epoch) this.failed()
      }
    } while (this.queued && this.active && this.visible)
    return received
  }
}
