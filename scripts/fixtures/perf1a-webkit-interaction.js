// Isolated synthetic IPC. Real React Interaction, no Rust/provider/network calls.
window.isTauri = true
window.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: () => {} }
const calls = [], callbacks = new Map(), tasks = new Map()
let nextCallback = 0, starts = 0, cancels = 0, operationalReads = 0, layoutWrites = 0
let shellSettings = { presentationMode: 'economy', layout: { leftOpen: true, leftWidth: 208, rightOpen: true, rightWidth: 272 } }
const messages = [{ id: 1, sessionId: 41, role: 'assistant', content: 'Fixture persistida', createdAt: '' }]
window.__TAURI_INTERNALS__ = {
  metadata: { currentWindow: { label: 'main' }, currentWebview: { label: 'main' } },
  transformCallback: fn => { callbacks.set(++nextCallback, fn); return nextCallback },
  unregisterCallback: id => callbacks.delete(id),
  invoke: async (cmd, args = {}) => {
    calls.push(cmd)
    if (cmd === 'get_shell_settings') return structuredClone(shellSettings)
    if (cmd === 'update_presentation_mode') { shellSettings.presentationMode = args.mode; return null }
    if (cmd === 'update_shell_layout') { layoutWrites++; shellSettings.layout = args.layout; return null }
    if (cmd === 'get_provider_operational_snapshot') { operationalReads++; return { capturedAtUnixMs: Date.now(), admission: [], telemetry: [], rate: [], resilience: [] } }
    if (cmd === 'get_general_settings') return { activeFps: 30, backgroundFps: 24, alwaysOnTop: false }
    if (cmd === 'conversation_routing_status') return { routingMode: 'fixed', targets: [{ providerId: 'fixture', displayName: 'Fixture', configured: true, cooldownMs: 0 }] }
    if (cmd === 'list_conversation_history') return []
    if (cmd === 'close_conversation_session') return true
    if (cmd === 'create_conversation_session') return 41
    if (cmd === 'get_conversation_session') return { id: 41, messages }
    if (cmd === 'start_conversation_task') {
      const taskId = 80 + ++starts, task = { taskId, channel: args.channel, sequence: 0 }
      tasks.set(taskId, task)
      task.channel.onmessage({ taskId, sequence: ++task.sequence, state: 'running', type: 'task_started' })
      task.channel.onmessage({ taskId, sequence: ++task.sequence, state: 'running', type: 'provider_selected', provider_id: 'fixture', attempt: 1, routing_reason: 'fixed', score: null })
      task.channel.onmessage({ taskId, sequence: ++task.sequence, state: 'running', type: 'provider_chunk', provider_id: 'fixture', chunk: 'prévia' })
      return taskId
    }
    if (cmd === 'cancel_task') {
      cancels++
      const task = tasks.get(args.taskId)
      if (!task) throw new Error('wrong cancellation TaskId')
      task.channel.onmessage({ taskId: task.taskId, sequence: ++task.sequence, state: 'cancelled', type: 'task_cancelled' })
      tasks.delete(args.taskId)
      return true
    }
    if (cmd.includes('is_always_on_top')) return false
    if (cmd.includes('outer_position')) return { x: 0, y: 0 }
    return 1
  },
}
// Track actual event/observer/RAF/timer registration and cancellation.
const visualListeners = new Map()
for (const [target, types] of [[window, ['focus', 'blur']], [document, ['visibilitychange']]]) {
  const add = target.addEventListener.bind(target), remove = target.removeEventListener.bind(target)
  for (const type of types) visualListeners.set(type, new Set())
  target.addEventListener = (type, fn, opts) => { visualListeners.get(type)?.add(fn); add(type, fn, opts) }
  target.removeEventListener = (type, fn, opts) => { visualListeners.get(type)?.delete(fn); remove(type, fn, opts) }
}
let observers = 0, failNextObserve = false
const NativeObserver = window.ResizeObserver
window.ResizeObserver = class extends NativeObserver {
  observed = false
  observe(...args) {
    if (!this.observed) { this.observed = true; observers++ }
    const result = super.observe(...args)
    if (failNextObserve) {
      failNextObserve = false
      throw new Error('PERF-1A FIX-1 injected failure after native ResizeObserver.observe')
    }
    return result
  }
  disconnect() { if (this.observed) { this.observed = false; observers-- } super.disconnect() }
}
const rafts = new Set(), intervals = new Map()
const nativeRAF = window.requestAnimationFrame.bind(window), nativeCancel = window.cancelAnimationFrame.bind(window)
window.requestAnimationFrame = fn => { const id = nativeRAF(at => { rafts.delete(id); fn(at) }); rafts.add(id); return id }
window.cancelAnimationFrame = id => { rafts.delete(id); nativeCancel(id) }
const nativeInterval = window.setInterval.bind(window), nativeClear = window.clearInterval.bind(window)
window.setInterval = (fn, ms, ...args) => { const id = nativeInterval(fn, ms, ...args); intervals.set(id, ms); return id }
window.clearInterval = id => { intervals.delete(id); nativeClear(id) }
window.__fixture = {
  failNextObserve: () => { failNextObserve = true },
  settings: () => structuredClone(shellSettings),
  metrics: () => ({ layoutWrites, operationalReads, starts, cancels, tasks: tasks.size, observers, rafs: rafts.size, intervals: [...intervals.values()].filter(ms => ms === 5000).length, otherIntervals: [...intervals.values()].filter(ms => ms !== 5000).length, listeners: [...visualListeners.values()].reduce((sum, set) => sum + set.size, 0) }),
  complete: () => {
    const task = [...tasks.values()][0]
    if (!task) throw new Error('missing task')
    messages.push({ id: 2, sessionId: 41, role: 'user', content: 'Fixture', createdAt: '' }, { id: 3, sessionId: 41, role: 'assistant', content: 'Fixture concluída', createdAt: '' })
    task.channel.onmessage({ taskId: task.taskId, sequence: ++task.sequence, state: 'completed', type: 'task_completed' })
    tasks.delete(task.taskId)
  },
}
