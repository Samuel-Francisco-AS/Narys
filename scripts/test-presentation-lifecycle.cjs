// Deterministic resource tests. Real avatar/scene/budget/director/adapter code,
// real Three resources, fake WebGLRenderer/DOM/GLTF transport; no GPU claims.
const assert = require('node:assert/strict')
const fs = require('node:fs'), path = require('node:path'), Module = require('node:module')
const ts = require('typescript')
;(async () => {
const three = await import('three')
const originalLoad = Module._load, originalTs = require.extensions['.ts']
const effects = [], refs = [], renders = [], pending = [], observers = []
let container, refIndex = 0, resources = [], fault = null
const timers = new Map(), timerCallbacks = [], canvases = []
let nextTimer = 0
function fail(point) {
  if (fault?.point !== point) return
  fault.acquired = { canvases: container.children.length, windowListeners: window.count(), documentListeners: document.count(), timers: timers.size }
  throw fault.error
}
function newContainer() { return { children: [], clientWidth: 300, clientHeight: 360, appendChild(c) { this.children.push(c); c.parent = this }, getBoundingClientRect() { fail('diagnostics-report'); return { width: 300, height: 360 } } } }
class Target extends EventTarget {
  registrations = new Map()
  addEventListener(type, fn, opts) { super.addEventListener(type, fn, opts); const set = this.registrations.get(type) ?? new Set(); set.add(fn); this.registrations.set(type, set) }
  removeEventListener(type, fn, opts) { super.removeEventListener(type, fn, opts); this.registrations.get(type)?.delete(fn) }
  count() { return [...this.registrations.values()].reduce((sum, set) => sum + set.size, 0) }
}
class Canvas extends Target {
  constructor() { super(); canvases.push(this) }
  setAttribute() {}
  remove() { if (this.parent) this.parent.children = this.parent.children.filter(c => c !== this); this.parent = null }
  getBoundingClientRect() { return { left: 0, top: 0, width: 300, height: 360 } }
}
class Renderer {
  disposed = 0; loop = null; renders = 0; stopped = 0; sizes = 0
  constructor({ canvas }) { this.domElement = canvas; renders.push(this) }
  setPixelRatio() { fail('renderer-config') } getPixelRatio() { return 1 } setSize() { this.sizes++ }
  getContext() { return { VERSION: 1, RENDERER: 2, NO_ERROR: 0, getParameter: () => { fail('renderer-context'); return 'test WebGL' }, getError: () => 0 } }
  setAnimationLoop(loop) { this.loop = loop; if (loop === null) { this.stopped++; if (fault?.cleanupThrows) throw new Error('secondary loop cleanup') } }
  render() { this.renders++ }
  dispose() { this.disposed++; if (fault?.cleanupThrows) throw new Error('secondary renderer cleanup') }
}
global.window = new Target(); window.devicePixelRatio = 1
window.setInterval = fn => { const id = ++nextTimer; timers.set(id, fn); timerCallbacks.push(fn); return id }
window.clearInterval = id => timers.delete(id)
global.document = new Target(); document.visibilityState = 'visible'; document.hasFocus = () => true; document.createElement = () => new Canvas()
global.ResizeObserver = class { connected = false; constructor(fn) { fail('observer-create'); this.fn = fn; observers.push(this) } observe() { this.connected = true; this.fn(); fail('observer-observe') } disconnect() { this.connected = false; if (fault?.cleanupThrows) throw new Error('secondary observer cleanup') } }
global.ImageBitmap = class { closed = 0; close() { this.closed++ } }
function gltf() {
  const root = new three.Group(), geometry = new three.BoxGeometry(1, 2, 1)
  const bitmap = new ImageBitmap(), texture = new three.Texture(bitmap), material = new three.MeshBasicMaterial({ map: texture })
  const mesh = new three.SkinnedMesh(geometry, material), skeleton = new three.Skeleton([new three.Bone()])
  geometry.computeBoundingBox(); mesh.boundingBox = geometry.boundingBox.clone()
  skeleton.computeBoneTexture(); mesh.skeleton = skeleton; root.add(mesh)
  const disposed = { geometry: 0, material: 0, texture: 0, bone: 0 }
  for (const [obj, key] of [[geometry, 'geometry'], [material, 'material'], [texture, 'texture'], [skeleton.boneTexture, 'bone']]) obj.addEventListener('dispose', () => disposed[key]++)
  resources.push({ disposed, bitmap, skeleton })
  return { scene: root, animations: [new three.AnimationClip('Idle', 1, []), new three.AnimationClip('Wave', 1, [])] }
}
Module._load = function(id, parent, isMain) {
  if (id === 'three') return { ...three, WebGLRenderer: Renderer }
  if (id === 'three/addons/loaders/GLTFLoader.js') return { GLTFLoader: class { load(url, loaded, _, error) { assert.equal(url, '/models/Luna.glb'); pending.push({ loaded, error }) } } }
  if (id === 'react/jsx-runtime') return { jsx: (type, props) => ({ type, props }) }
  if (id === 'react') return { useRef: value => { const ref = { current: refIndex++ === 0 ? container : value }; refs.push(ref); return ref }, useEffect: fn => effects.push(fn) }
  return originalLoad.call(this, id, parent, isMain)
}
require.extensions['.ts'] = require.extensions['.tsx'] = (mod, filename) => {
  const source = fs.readFileSync(filename, 'utf8').replaceAll('import.meta.env.DEV', 'global.__lifecycleDiagnostics === true')
  mod._compile(ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, jsx: ts.JsxEmit.ReactJSX } }).outputText, filename)
}
try {
  const { PresentationController } = require('../src/presentation/PresentationController.ts')
  const controller = new PresentationController(), states = []
  const unsubscribe = controller.subscribe(() => states.push(controller.getSnapshot()))
  controller.report(1, 'ready'); controller.setMode('economy'); controller.report(1, 'ready')
  assert.equal(controller.getSnapshot().phase, 'detached')
  controller.setMode('economy'); assert.equal(states.length, 2)
  controller.setMode('presence'); controller.report(1, 'error'); assert.equal(controller.getSnapshot().phase, 'loading')
  controller.report(3, 'error'); controller.recreatePresence(); assert.equal(controller.getSnapshot().generation, 4)
  controller.setMode('headless'); assert.equal(controller.getSnapshot().phase, 'detached')
  assert.throws(() => controller.setMode('auto'), /invalid/); unsubscribe()
  const Viewport = require('../src/avatar/AvatarViewport.tsx').default
  for (let cycle = 0; cycle < 20; cycle++) {
    effects.length = 0; refs.length = 0; refIndex = 0
    container = newContainer()
    const statuses = []
    Viewport({ animationRequest: null, onReadyChange: v => statuses.push(v), onStatusChange: s => statuses.push(s), renderConfig: { activeFps: 30, backgroundFps: 24 } })
    const cleanups = effects.map(fn => fn()), renderer = renders.at(-1), canvas = renderer.domElement
    assert.equal(container.children.length, 1); assert.equal(canvas.count(), 3)
    assert.equal(window.count(), 2); assert.equal(document.count(), 1)
    const loader = pending.shift(), late = cycle % 2 === 1
    if (!late) {
      loader.loaded(gltf()); assert(statuses.includes(true)); assert(refs[1].current)
      refs[1].current.request({ type: 'greeting' }); renderer.loop(); assert.equal(renderer.renders, 1)
    }
    const director = refs[1].current
    let stopped = 0, uncached = 0
    if (director) {
      const stop = director.mixer.stopAllAction.bind(director.mixer), uncache = director.mixer.uncacheRoot.bind(director.mixer)
      director.mixer.stopAllAction = () => { stopped++; stop() }; director.mixer.uncacheRoot = root => { uncached++; uncache(root) }
    }
    cleanups.forEach(fn => fn?.())
    assert.equal(stopped, late ? 0 : 1); assert.equal(uncached, late ? 0 : 1)
    assert.equal(renderer.loop, null); assert(renderer.stopped >= 1); assert.equal(renderer.disposed, 1)
    assert.equal(container.children.length, 0); assert.equal(canvas.count(), 0)
    assert.equal(window.count(), 0); assert.equal(document.count(), 0); assert(!observers.at(-1).connected)
    assert.equal(refs[1].current, null); assert.equal(refs[2].current, null)
    const before = statuses.length
    canvas.dispatchEvent(new Event('webglcontextlost')); window.dispatchEvent(new Event('blur')); document.dispatchEvent(new Event('visibilitychange'))
    if (late) { loader.loaded(gltf()); loader.error(new Error('late error')) }
    assert.equal(statuses.length, before)
    assert.deepEqual(resources.at(-1).disposed, { geometry: 1, material: 1, texture: 1, bone: 1 }); assert.equal(resources.at(-1).bitmap.closed, 1); assert.equal(resources.at(-1).skeleton.boneTexture, null)
  }
  assert(renders.every(renderer => renderer.loop === null && renderer.disposed === 1))
  assert(observers.every(observer => !observer.connected))
  // Faults occur after acquisition, including an observer that observes before
  // throwing. DEV diagnostics are active, unlike the original production cycles.
  global.__lifecycleDiagnostics = true
  const { SceneRuntime } = require('../src/avatar/runtime/SceneRuntime.ts')
  const failures = [
    { point: 'renderer-config' },
    { point: 'renderer-context' },
    { point: 'diagnostics-report' },
    { point: 'observer-create' },
    { point: 'observer-observe' },
    { point: 'observer-observe', cleanupThrows: true },
  ]
  for (const injected of failures) {
    container = newContainer()
    const statuses = [], originalError = new Error('original init: ' + injected.point)
    const observerCount = observers.length, timerCount = timerCallbacks.length
    fault = { ...injected, error: originalError }
    const activeFault = fault
    const errorLog = console.error
    // Even secondary error reporting must not replace the original exception.
    if (injected.cleanupThrows) console.error = () => { throw new Error('secondary logging') }
    try {
      assert.throws(() => new SceneRuntime(container, { onStatusChange: s => statuses.push(s), onReadyChange: s => statuses.push(s) }), error => error === originalError)
    } finally { console.error = errorLog; fault = null }
    assert.equal(activeFault.acquired.canvases, injected.point === 'renderer-config' ? 0 : 1)
    if (injected.point.startsWith('observer-')) assert.deepEqual(activeFault.acquired, { canvases: 1, windowListeners: 2, documentListeners: 1, timers: 1 })
    const failedRenderer = renders.at(-1), failedCanvas = canvases.at(-1)
    assert.equal(failedRenderer.disposed, 1); assert.equal(failedRenderer.loop, null); assert(failedRenderer.stopped >= 1)
    assert.equal(container.children.length, 0); assert.equal(failedCanvas.count(), 0)
    assert.equal(window.count(), 0); assert.equal(document.count(), 0); assert.equal(timers.size, 0)
    const failedObservers = observers.slice(observerCount), lateTimers = timerCallbacks.slice(timerCount)
    assert(failedObservers.every(observer => !observer.connected))
    if (injected.point.startsWith('observer-')) assert.equal(lateTimers.length, 1, 'diagnostics timer must actually have been acquired')
    const sizes = failedRenderer.sizes
    const deliverFailedCallbacks = () => {
      failedObservers.forEach(observer => observer.fn())
      const info = console.info; let lateReports = 0
      console.info = () => lateReports++
      try { lateTimers.forEach(fn => fn()) } finally { console.info = info }
      failedCanvas.dispatchEvent(new Event('webglcontextlost')); failedCanvas.dispatchEvent(new Event('webglcontextrestored'))
      assert.equal(failedRenderer.sizes, sizes); assert.equal(lateReports, 0); assert.equal(statuses.length, 0)
    }
    deliverFailedCallbacks()
    window.dispatchEvent(new Event('blur')); window.dispatchEvent(new Event('focus')); document.dispatchEvent(new Event('visibilitychange'))
    assert.equal(statuses.length, 0)
    const retry = new SceneRuntime(container, { onStatusChange() {}, onReadyChange() {} })
    const renderer = renders.at(-1); let frames = 0
    retry.start(() => frames++); const staleLoop = renderer.loop; staleLoop()
    assert.equal(renderer.renders, 1); assert.equal(container.children.length, 1); assert.equal(timers.size, 1)
    const retrySizes = renderer.sizes
    deliverFailedCallbacks()
    assert.equal(renderer.sizes, retrySizes); assert.equal(window.count(), 2); assert.equal(document.count(), 1); assert.equal(timers.size, 1)
    retry.dispose(); retry.dispose(); retry.start(() => frames++)
    staleLoop(); observers.at(-1).fn()
    assert.equal(frames, 1); assert.equal(renderer.disposed, 1); assert.equal(renderer.loop, null)
    assert.equal(container.children.length, 0); assert.equal(timers.size, 0); assert.equal(window.count(), 0); assert.equal(document.count(), 0)
  }
  // Actual AvatarViewport constructor catch + Presentation generation protocol.
  const retryController = new PresentationController()
  effects.length = 0; refs.length = 0; refIndex = 0; container = newContainer()
  const report = ready => retryController.report(retryController.getSnapshot().generation, ready ? 'ready' : 'error')
  const viewportProps = { animationRequest: null, onReadyChange: report, onStatusChange() {}, renderConfig: { activeFps: 30, backgroundFps: 24 } }
  fault = { point: 'observer-observe', error: new Error('Presence partial init') }
  const errorLog = console.error; console.error = () => {}
  let failedCleanups
  try { Viewport(viewportProps); failedCleanups = effects.map(fn => fn()) } finally { fault = null; console.error = errorLog }
  assert.equal(retryController.getSnapshot().phase, 'error'); assert.equal(pending.length, 0)
  assert.equal(refs[1].current, null); assert.equal(refs[2].current, null)
  failedCleanups.forEach(fn => fn?.())
  assert.equal(container.children.length, 0); assert.equal(timers.size, 0); assert.equal(window.count(), 0); assert.equal(document.count(), 0)
  const failedGeneration = retryController.getSnapshot().generation
  retryController.recreatePresence(); assert(retryController.getSnapshot().generation > failedGeneration)
  effects.length = 0; refs.length = 0; refIndex = 0
  Viewport(viewportProps); const retryCleanups = effects.map(fn => fn())
  pending.shift().loaded(gltf()); assert.equal(retryController.getSnapshot().phase, 'ready')
  retryCleanups.forEach(fn => fn?.())
  assert(renders.every(renderer => renderer.loop === null && renderer.disposed === 1))
  assert(observers.every(observer => !observer.connected)); assert.equal(timers.size, 0)
  assert.equal(window.count(), 0); assert.equal(document.count(), 0); assert.equal(container.children.length, 0)
  const assets = fs.readdirSync('dist/assets'), avatar = assets.find(name => /^AvatarViewport-.*\.js$/.test(name)), app = assets.find(name => /^App-.*\.js$/.test(name))
  assert(avatar && app, 'Build must split AvatarViewport from App')
  const avatarSource = fs.readFileSync(path.join('dist/assets', avatar), 'utf8'), appSource = fs.readFileSync(path.join('dist/assets', app), 'utf8')
  assert(avatarSource.includes('WebGLRenderer')); assert(!['WebGLRenderer', 'GLTFLoader', 'boneTexture', '/models/Luna.glb'].some(marker => appSource.includes(marker)));  assert(appSource.includes('import(')); assert(!appSource.includes('__narysPerf1A'))
  console.log('PERF-1A FIX-1: 6 partial-init cases PASS (including cleanup/logging failures), original error, stale callbacks, fresh retries and Presentation error/retry/ready; 20 cycles PASS; real resource disposal incl. late loads, mixer, listener/observer/loop/canvas/ref cleanup; stale lifecycle reports; production chunk boundary and DEV stripping.')
} finally { Module._load = originalLoad; require.extensions['.ts'] = originalTs; delete require.extensions['.tsx']; delete global.__lifecycleDiagnostics }

})().catch(error => { console.error(error); process.exitCode = 1 })
