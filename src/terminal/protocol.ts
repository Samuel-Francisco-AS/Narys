export interface Session { sessionId: string; shell: string; startingDirectory: string; state: string; rows: number; cols: number; exitCode: number | null; reaped: boolean }
export interface TraceEvent { sequence: string; lastSequence: string; fragments: number; observedAtUnixMs: number; lastObservedAtUnixMs: number; class: 'STREAM' | 'STATE' | 'CRITICAL'; sourceType: string; sourceId: string; sourceInstance: string | null; taskId: number | null; subtaskId: string | null; correlationId: string | null; kind: string; code: string | null; channel: string | null; text: string }
export interface TraceBatch { cursor: string; missingEvents: string; liveDeliveryDropped: string; replayComplete: boolean; events: TraceEvent[] }
export const RAW_HEADER_BYTES = 24
export function decodePtyFrame(buffer: ArrayBuffer) {
  if (!(buffer instanceof ArrayBuffer) || buffer.byteLength < RAW_HEADER_BYTES) throw new Error('Frame PTY inválido')
  const header = new DataView(buffer)
  return { cursor: header.getBigUint64(0, true).toString(), missingChunks: header.getBigUint64(8, true), droppedBytes: header.getBigUint64(16, true), bytes: new Uint8Array(buffer, RAW_HEADER_BYTES) }
}
// Mirrors LR-9B. Small keystrokes share 8ms; paste splits into native-safe units.
export const MAX_INPUT_BYTES = 64 * 1024
export const INPUT_BATCH_MS = 8
export const INPUT_THRESHOLD_BYTES = 4096
export const INPUT_PENDING_BYTES = 256 * 1024
export class InputBatcher {
  private buffers: Uint8Array[] = []
  private bytes = 0
  private timer: ReturnType<typeof setTimeout> | undefined
  private sending = false
  private stopped = false
  private blocked = false
  constructor(private send: (bytes: Uint8Array) => Promise<void>, private failed: (message: string) => void) {}
  text(text: string) { this.push(new TextEncoder().encode(text)) }
  binary(text: string) { this.push(Uint8Array.from(text, c => c.charCodeAt(0) & 255)) }
  push(bytes: Uint8Array) {
    if (this.stopped || this.blocked || bytes.length === 0) return
    if (this.bytes + bytes.length > INPUT_PENDING_BYTES) { this.failed('Entrada excede 256 KiB pendentes; cole em partes menores.'); return }
    this.buffers.push(bytes); this.bytes += bytes.length
    if (this.bytes >= INPUT_THRESHOLD_BYTES) { if (this.timer) clearTimeout(this.timer); this.timer = undefined; void this.flush() }
    else if (!this.timer) this.timer = setTimeout(() => { this.timer = undefined; void this.flush() }, INPUT_BATCH_MS)
  }
  private take() {
    const output = new Uint8Array(Math.min(this.bytes, MAX_INPUT_BYTES)); let offset = 0
    while (offset < output.length) {
      const first = this.buffers[0], n = Math.min(first.length, output.length - offset)
      output.set(first.subarray(0, n), offset); offset += n; this.bytes -= n
      if (n === first.length) this.buffers.shift(); else this.buffers[0] = first.subarray(n)
    }
    return output
  }
  async flush() {
    if (this.sending || this.stopped || this.blocked) return
    this.sending = true
    try {
      while (this.bytes && !this.stopped) await this.send(this.take())
    } catch (error) {
      // No automatic retry: transport errors can have an unknown outcome.
      this.buffers = []; this.bytes = 0; this.blocked = true
      this.failed(`Entrada interrompida (${String(error)}). Pendências descartadas; retome e digite novamente. Nenhum retry automático.`)
    } finally { this.sending = false }
  }
  resume() { this.blocked = false }
  dispose() { this.stopped = true; this.buffers = []; this.bytes = 0; if (this.timer) clearTimeout(this.timer) }
}
export const RESIZE_DEBOUNCE_MS = 80
export class ResizeCoalescer {
  private timer: ReturnType<typeof setTimeout> | undefined
  private latest: { rows: number; cols: number } | undefined
  private last = ''
  private sending = false
  private stopped = false
  constructor(private send: (rows: number, cols: number) => Promise<void>, private failed: (message: string) => void) {}
  resize(rows: number, cols: number) {
    this.latest = { rows: Math.max(1, Math.min(1000, rows)), cols: Math.max(1, Math.min(1000, cols)) }
    if (this.timer) clearTimeout(this.timer)
    this.timer = setTimeout(() => { this.timer = undefined; void this.flush() }, RESIZE_DEBOUNCE_MS)
  }
  private async flush() {
    if (this.sending || this.stopped || !this.latest) return
    this.sending = true
    try {
      while (this.latest && !this.stopped) {
        const size = this.latest; this.latest = undefined; const key = `${size.rows}:${size.cols}`
        if (key !== this.last) { await this.send(size.rows, size.cols); this.last = key }
      }
    } catch (e) { this.failed(`Resize indisponível: ${String(e)}`) } finally { this.sending = false }
  }
  dispose() { this.stopped = true; if (this.timer) clearTimeout(this.timer) }
}
