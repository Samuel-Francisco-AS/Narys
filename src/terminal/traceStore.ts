import type { TraceBatch, TraceEvent } from './protocol'
export const TRACE_MAX_ROWS = 1024
export const TRACE_MAX_BYTES = 2 * 1024 * 1024
export const TRACE_UPDATE_MS = 50
export const TRACE_ROW_HEIGHT = 26
export const TRACE_OVERSCAN = 6
export const TRACE_MAX_DOM_ROWS = 40
export interface TraceSnapshot { events: readonly TraceEvent[]; missing: bigint; evicted: bigint; liveDropped: bigint; incomplete: boolean; updates: number }
export class TraceStore {
  private rows: TraceEvent[] = []
  private sizes: number[] = []
  private bytes = 0
  private cursor = 0n
  private missing = 0n
  private evicted = 0n
  private liveDropped = 0n
  private incomplete = false
  private updates = 0
  private listeners = new Set<() => void>()
  private timer: ReturnType<typeof setTimeout> | undefined
  private afterFlush: (() => void)[] = []
  private snapshot: TraceSnapshot = { events: [], missing: 0n, evicted: 0n, liveDropped: 0n, incomplete: false, updates: 0 }
  subscribe = (listener: () => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener) } }
  getSnapshot = () => this.snapshot
  ingest(batch: TraceBatch, acknowledge: () => void) {
    // Exactly one native in-flight batch. Tests may simulate multiple arrivals;
    // keep callbacks bounded as well and never create an event-level render.
    if (this.afterFlush.length >= 8) throw new Error('Activity delivery excede budget')
    for (const event of batch.events) {
      if (BigInt(event.lastSequence) <= this.cursor) continue
      const size = 512 + new TextEncoder().encode(event.text).length
      this.rows.push(event); this.sizes.push(size); this.bytes += size
      while (this.rows.length > TRACE_MAX_ROWS || this.bytes > TRACE_MAX_BYTES) {
        let index = this.rows.findIndex(e => e.class === 'STREAM')
        if (index < 0) index = this.rows.findIndex(e => e.class === 'STATE')
        if (index < 0) index = 0
        this.evicted += BigInt(this.rows[index].fragments)
        this.bytes -= this.sizes[index]; this.rows.splice(index, 1); this.sizes.splice(index, 1)
      }
    }
    this.cursor = BigInt(batch.cursor); this.missing += BigInt(batch.missingEvents)
    this.liveDropped = BigInt(batch.liveDeliveryDropped); this.incomplete ||= !batch.replayComplete
    this.afterFlush.push(acknowledge)
    if (!this.timer) this.timer = setTimeout(() => this.flush(), TRACE_UPDATE_MS)
  }
  flush() {
    if (this.timer) clearTimeout(this.timer); this.timer = undefined
    this.snapshot = { events: this.rows.slice(), missing: this.missing, evicted: this.evicted, liveDropped: this.liveDropped, incomplete: this.incomplete, updates: ++this.updates }
    for (const listener of this.listeners) listener()
    const callbacks = this.afterFlush; this.afterFlush = []; for (const callback of callbacks) callback()
  }
  dispose() { if (this.timer) clearTimeout(this.timer); this.listeners.clear(); this.afterFlush = [] }
}
export function filterTrace(events: readonly TraceEvent[], source: string, task: string, kind: string) {
  return events.filter(e => (!source || `${e.sourceType}:${e.sourceId}` === source) && (!task || String(e.taskId) === task) && (!kind || e.class === kind))
}
export function virtualRange(length: number, scrollTop: number, height: number) {
  const first = Math.min(Math.max(0, length - 1), Math.max(0, Math.floor(scrollTop / TRACE_ROW_HEIGHT) - TRACE_OVERSCAN))
  const count = Math.min(TRACE_MAX_DOM_ROWS, Math.ceil(height / TRACE_ROW_HEIGHT) + 2 * TRACE_OVERSCAN)
  return { first, end: Math.min(length, first + count) }
}
