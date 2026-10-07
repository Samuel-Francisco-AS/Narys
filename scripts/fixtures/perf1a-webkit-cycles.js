;(async () => {
  const wait = ms => new Promise(resolve => setTimeout(resolve, ms))
  const assert = (value, detail) => { if (!value) throw new Error(detail) }
  const until = async (predicate, detail) => { const start = performance.now(); while (!predicate()) { if (performance.now() - start > 20000) throw new Error(detail); await wait(50) } }
  await until(() => window.__narysPerf1A?.snapshot().presentation.phase === 'ready', 'Initial Presence not ready')
  const harness = window.__narysPerf1A, before = harness.snapshot()
  document.querySelector('.presence-handle').click()
  await until(() => document.querySelector('textarea'), 'composer missing')
  const send = async () => {
    const input = document.querySelector('textarea')
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set.call(input, 'Fixture')
    input.dispatchEvent(new Event('input', { bubbles: true }))
    await wait(50)
    document.querySelector('.send-button').click()
    await until(() => harness.snapshot().conversation.activeTaskId !== null, 'task missing')
  }
  await send()
  const running = harness.snapshot().conversation, samples = []
  for (let cycle = 0; cycle < 20; cycle++) {
    harness.setMode(cycle % 2 ? 'headless' : 'economy')
    await until(() => harness.snapshot().canvases === 0, 'canvas survives detach')
    await wait(50)
    const metrics = window.__fixture.metrics(), state = harness.snapshot().conversation
    assert(metrics.observers === 0 && metrics.rafs === 0 && metrics.intervals === 0 && metrics.listeners === 0, 'visual registrations survive detach: ' + JSON.stringify(metrics))
    assert(state.sessionId === running.sessionId && state.activeTaskId === running.activeTaskId && state.assistantStreaming && state.providerRoute === running.providerRoute, 'Interaction changed during detach')
    assert(metrics.starts === 1 && metrics.cancels === 0, 'task restarted/cancelled')
    harness.setMode('presence')
    await until(() => harness.snapshot().presentation.phase === 'ready', 'Presence reentry failed')
    await wait(100)
    const attached = window.__fixture.metrics()
    assert(harness.snapshot().canvases === 1 && attached.observers === 1 && attached.listeners === 3 && attached.intervals === 1 && attached.rafs <= 1, 'duplicate visual registrations: ' + JSON.stringify(attached))
    samples.push({ cycle: cycle + 1, detached: metrics, attached, state: harness.snapshot() })
  }
  harness.setMode('economy'); await until(() => harness.snapshot().canvases === 0, 'final detach')
  window.__fixture.complete()
  await until(() => !harness.snapshot().conversation.assistantStreaming, 'completion lost')
  assert(harness.snapshot().conversation.messageCount === 3, 'persisted fixture not refreshed')
  await send()
  const cancelling = harness.snapshot().conversation.activeTaskId
  harness.setMode('presence'); await until(() => harness.snapshot().presentation.phase === 'ready', 'cancel reentry')
  const button = [...document.querySelectorAll('button')].find(button => button.textContent === 'Cancelar')
  assert(button, 'cancellation control missing'); button.click()
  await until(() => harness.snapshot().conversation.activeTaskId === null, 'cancellation lost')
  assert(window.__fixture.metrics().starts === 2 && window.__fixture.metrics().cancels === 1, 'duplicate calls/cancellation')
  // Exercise deterministic runtime error teardown and the real retry control.
  const nativeContext = HTMLCanvasElement.prototype.getContext
  HTMLCanvasElement.prototype.getContext = function(type, ...args) {
    if (type.startsWith('webgl')) throw new Error('PERF-1A injected WebGL failure')
    return nativeContext.call(this, type, ...args)
  }
  harness.recreate()
  await until(() => harness.snapshot().presentation.phase === 'error' && harness.snapshot().canvases === 0, 'runtime failure not surfaced/cleaned')
  HTMLCanvasElement.prototype.getContext = nativeContext
  const retry = [...document.querySelectorAll('button')].find(button => button.textContent === 'Tentar novamente')
  assert(retry, 'retry control missing'); retry.click()
  await until(() => harness.snapshot().presentation.phase === 'ready', 'runtime retry failed')
  window.webkit.messageHandlers.perf.postMessage(JSON.stringify({ type: 'lifecycle-result', pass: true, before, running, cancelling, final: harness.snapshot(), metrics: window.__fixture.metrics(), samples }))
})().catch(error => window.webkit.messageHandlers.perf.postMessage(JSON.stringify({ type: 'lifecycle-result', pass: false, error: String(error), stack: error.stack })))
