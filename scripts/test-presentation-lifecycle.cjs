// Deterministic resource tests. Real avatar/scene/budget/director/adapter code,
// real Three resources, fake WebGLRenderer/DOM/GLTF transport; no GPU claims.
const assert = require('node:assert/strict')
const fs = require('node:fs'), path = require('node:path'), Module = require('node:module')
const ts = require('typescript')
;(async () => {
const three = await import('three')
const originalLoad = Module._load, originalTs = require.extensions['.ts']
const effects = [], refs = [], renders = [], pending = [], observers = []
let container, refIndex = 0, resources = []
class Target extends EventTarget {
  registrations = new Map()
  addEventListener(type, fn, opts) { super.addEventListener(type, fn, opts); const set = this.registrations.get(type) ?? new Set(); set.add(fn); this.registrations.set(type, set) }
  removeEventListener(type, fn, opts) { super.removeEventListener(type, fn, opts); this.registrations.get(type)?.delete(fn) }
  count() { return [...this.registrations.values()].reduce((sum, set) => sum + set.size, 0) }
}
class Canvas extends Target {
  setAttribute() {}
  remove() { container.children = container.children.filter(c => c !== this) }
  getBoundingClientRect() { return { left: 0, top: 0, width: 300, height: 360 } }
}
class Renderer {
  disposed = 0; loop = null; renders = 0; stopped = 0
  constructor({ canvas }) { this.domElement = canvas; renders.push(this) }
  setPixelRatio() {} setSize() {}
  getContext() { return { VERSION: 1, RENDERER: 2, NO_ERROR: 0, getParameter: () => 'test WebGL', getError: () => 0 } }
  setAnimationLoop(loop) { this.loop = loop; if (loop === null) this.stopped++ }
  render() { this.renders++ }
  dispose() { this.disposed++ }
}
global.window = new Target(); window.devicePixelRatio = 1
global.document = new Target(); document.visibilityState = 'visible'; document.hasFocus = () => true; document.createElement = () => new Canvas()
global.ResizeObserver = class { connected = false; constructor(fn) { this.fn = fn; observers.push(this) } observe() { this.connected = true; this.fn() } disconnect() { this.connected = false } }
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
  const source = fs.readFileSync(filename, 'utf8').replaceAll('import.meta.env.DEV', 'false')
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
    container = { children: [], clientWidth: 300, clientHeight: 360, appendChild(c) { this.children.push(c) } }
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
  const assets = fs.readdirSync('dist/assets'), avatar = assets.find(name => /^AvatarViewport-.*\.js$/.test(name)), app = assets.find(name => /^App-.*\.js$/.test(name))
  assert(avatar && app, 'Build must split AvatarViewport from App')
  const avatarSource = fs.readFileSync(path.join('dist/assets', avatar), 'utf8'), appSource = fs.readFileSync(path.join('dist/assets', app), 'utf8')
  assert(avatarSource.includes('WebGLRenderer')); assert(!['WebGLRenderer', 'GLTFLoader', 'boneTexture', '/models/Luna.glb'].some(marker => appSource.includes(marker)));  assert(appSource.includes('import(')); assert(!appSource.includes('__narysPerf1A'))
  console.log('PERF-1A: 20 cycles PASS; real resource disposal incl. late loads, mixer, listener/observer/loop/canvas/ref cleanup; stale lifecycle reports; production chunk boundary and DEV stripping.')
} finally { Module._load = originalLoad; require.extensions['.ts'] = originalTs; delete require.extensions['.tsx'] }

})().catch(error => { console.error(error); process.exitCode = 1 })
