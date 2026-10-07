;(async () => {
  const wait = ms => new Promise(r => setTimeout(r, ms))
  const assert = (v, m) => { if (!v) throw new Error(m) }
  const until = async (fn, m) => { const start = performance.now(); while (!fn()) { if (performance.now() - start > 25000) throw new Error(m); await wait(50) } }
  const click = label => { const button = document.querySelector(`[aria-label="${label}"]`); assert(button, `missing ${label}`); button.click() }
  const textClick = text => { const button = [...document.querySelectorAll('button')].find(b => b.textContent === text); assert(button, `missing ${text}`); button.click() }
  const send = async () => {
    const input = document.querySelector('textarea')
    Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value').set.call(input, 'Fixture')
    input.dispatchEvent(new Event('input', { bubbles: true })); await wait(50)
    textClick('Enviar'); await until(() => window.__fixture.metrics().tasks === 1, 'conversation task missing')
  }
  await until(() => document.querySelector('.economy-shell') && !document.querySelector('.nav-collapse').disabled, 'Economy hydration')
  await wait(1200)
  assert(!window.__narysPerf1A, 'DEV harness shipped in production')
  assert(document.querySelectorAll('canvas').length === 0 && window.__graphics.contexts === 0 && window.__graphics.frames === 0, '3D at Economy boot')
  assert(!performance.getEntriesByType('resource').some(e => /AvatarViewport|Luna\.glb/.test(e.name)), '3D resource at Economy boot')
  const boot = { atMs: performance.now(), contexts: window.__graphics.contexts, frames: window.__graphics.frames, metrics: window.__fixture.metrics() }
  await send()
  await until(() => document.querySelector('.conversation-messages').textContent.includes('prévia'), 'streaming missing')
  textClick('Cancelar'); await until(() => window.__fixture.metrics().tasks === 0, 'cancel failed')
  assert(window.__fixture.metrics().cancels === 1 && window.__graphics.contexts === 0, 'cancel imported Presence')
  await send(); window.__fixture.complete()
  await until(() => document.querySelector('.conversation-messages').textContent.includes('Fixture concluída'), 'completion/history refresh')
  click('Recolher painel operacional'); await wait(150)
  const reads = window.__fixture.metrics().operationalReads
  await wait(2200)
  assert(window.__fixture.metrics().operationalReads === reads, 'collapsed panel keeps polling')
  assert(window.__fixture.settings().layout.rightOpen === false, 'right preference not persisted')
  click('Recolher navegação'); await wait(100)
  assert(window.__fixture.settings().layout.leftOpen === false, 'left preference not persisted')
  click('Expandir navegação'); await wait(100)
  const splitter = document.querySelector('[aria-label="Redimensionar lateral esquerda"]')
  for (let i=0;i<30;i++) { splitter.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true })); await wait(25) }
  await wait(200)
  assert(window.__fixture.settings().layout.leftWidth === 320, 'resize clamp missing')
  const writes = window.__fixture.metrics().layoutWrites, capture = splitter.setPointerCapture
  splitter.setPointerCapture = () => {} // Synthetic pointer ID; handlers/layout/persistence remain real.
  splitter.dispatchEvent(new PointerEvent('pointerdown', {button:0,pointerId:7,clientX:200,bubbles:true}))
  for(const x of [190,180,170]) splitter.dispatchEvent(new PointerEvent('pointermove',{pointerId:7,clientX:x,bubbles:true}))
  await wait(100)
  assert(window.__fixture.metrics().layoutWrites === writes, 'resize writes each pixel')
  splitter.dispatchEvent(new PointerEvent('pointerup',{pointerId:7,clientX:160,bubbles:true}))
  splitter.setPointerCapture = capture
  await wait(100)
  assert(window.__fixture.metrics().layoutWrites === writes+1 && window.__fixture.settings().layout.leftWidth === 280, 'resize commit failed')

  await send()
  click('Settings'); await until(() => document.querySelector('[data-view="settings"]'), 'settings view missing'); textClick('Ativar Presence 3D')
  await until(() => document.querySelector('[data-presentation-phase="ready"]'), 'Presence opt-in failed')
  assert(document.querySelectorAll('canvas').length === 1 && window.__graphics.contexts > 0, 'Presence renderer missing')
  assert(window.__fixture.settings().presentationMode === 'presence', 'Presence preference missing')
  document.querySelector('.presence-handle').click()
  await until(() => document.querySelector('textarea'), 'Presence composer missing')
  assert(window.__fixture.metrics().tasks === 1 && window.__fixture.metrics().starts === 3, 'Interaction lost on Presence')
  await wait(1200)
  assert(performance.getEntriesByType('resource').some(e => /Luna\.glb/.test(e.name)), 'GLB not loaded')
  assert(window.__graphics.frames3D > 0, 'Presence does not render frames')
  textClick('Economy'); await until(() => document.querySelector('.economy-shell'), 'return Economy failed')
  await wait(100)
  const detachedFrames = window.__graphics.frames
  await wait(1200)
  assert(document.querySelectorAll('canvas').length === 0 && window.__graphics.frames === detachedFrames && window.__fixture.metrics().observers === 0, 'teardown failed')
  assert(window.__fixture.settings().presentationMode === 'economy', 'Economy preference missing')
  click('Conversa')
  await until(() => document.querySelector('.conversation-messages'), 'conversation view')
  assert(window.__fixture.metrics().tasks === 1 && window.__fixture.metrics().starts === 3, 'task restarted on detach')
  window.__fixture.complete()
  await wait(150)
  assert(document.querySelector('.conversation-messages').textContent.includes('Fixture concluída'), 'conversation lost during switch')
  textClick('Histórico'); await until(() => document.querySelector('.history-list'), 'history inaccessible')
  textClick('← Voltar'); await until(() => [...document.querySelectorAll('button')].some(b => b.textContent === 'Nova conversa'), 'current view'); textClick('Nova conversa'); await wait(150)
  assert(window.__fixture.metrics().starts === 3 && window.__fixture.metrics().cancels === 1, 'mode switch changed task semantics')
  window.webkit.messageHandlers.perf.postMessage(JSON.stringify({ type: 'shell-result', pass: true, boot, final: { settings: window.__fixture.settings(), frames: window.__graphics.frames, contexts: window.__graphics.contexts, metrics: window.__fixture.metrics() } }))
})().catch(e => window.webkit.messageHandlers.perf.postMessage(JSON.stringify({type:'shell-result',pass:false,error:String(e),stack:e.stack})))
