/** One timeout and at most one IPC in flight, including React StrictMode restart.
 * Only the last snapshot is owned by the caller. Hidden/disposed responses are
 * ignored; manual refresh during IPC coalesces one fresh read after it finishes. */
export class OperationalPoller<T> {
  private active = false
  private visible = false
  private epoch = 0
  private pending: Promise<boolean> | null = null
  private queued = false
  private manualEpoch: number | null = null
  private manualFeedbackActive = false
  private timer: ReturnType<typeof setTimeout> | undefined
  constructor(private readonly read: () => Promise<T>, private readonly receive: (value: T) => void,
    private readonly failed: () => void, private readonly manualBusy: (value: boolean) => void,
    private readonly intervalMs = 1000) {}
  start(visible: boolean) { this.active = true; this.setVisible(visible) }
  stop() { this.active = false; this.visible = false; this.epoch++; this.queued = false; this.clearTimer() }
  setVisible(visible: boolean) {
    this.visible = visible; this.epoch++; this.clearTimer()
    this.manualEpoch = null
    if (this.active && visible) {
      // Clear feedback from an interrupted manual action only in a live lifecycle.
      if (this.manualFeedbackActive) { this.manualFeedbackActive = false; this.manualBusy(false) }
      void this.refresh()
    }
  }
  private clearTimer() { clearTimeout(this.timer); this.timer = undefined }
  /** Only an explicit button action owns visual feedback. The shared drain
   * includes a fresh read queued behind any pending automatic IPC. */
  refreshManual(): Promise<boolean> {
    if (!this.active || !this.visible) return Promise.resolve(false)
    const epoch = this.epoch
    if (this.manualEpoch !== epoch) {
      this.manualEpoch = epoch; this.manualFeedbackActive = true; this.manualBusy(true)
    }
    return this.refresh().finally(() => {
      if (this.active && this.visible && this.epoch === epoch && this.manualEpoch === epoch) {
        this.manualEpoch = null; this.manualFeedbackActive = false; this.manualBusy(false)
      }
    })
  }
  /** Automatic and post-policy refreshes remain visually silent. */
  refresh(): Promise<boolean> {
    if (!this.active || !this.visible) return Promise.resolve(false)
    this.clearTimer()
    if (this.pending) { this.queued = true; return this.pending }
    this.pending = this.drain().finally(() => {
      this.pending = null
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
