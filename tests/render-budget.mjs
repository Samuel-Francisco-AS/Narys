import assert from 'node:assert/strict'
import { RenderBudget } from '../src/avatar/runtime/RenderBudget.ts'
const listeners = new Map()
const target = { addEventListener: (name, fn) => listeners.set(name, fn), removeEventListener: name => listeners.delete(name) }
let focused = true
Object.assign(globalThis, {
  document: { ...target, visibilityState: 'visible', hasFocus: () => focused },
  window: { ...target },
})
const budget = new RenderBudget(() => {})
assert.equal(budget.current.targetFps, 30)
focused = false
listeners.get('blur')()
assert.equal(budget.current.targetFps, 24)
budget.updateConfig({ activeFps: 45, backgroundFps: 20 })
assert.equal(budget.current.targetFps, 20)
focused = true
listeners.get('focus')()
assert.equal(budget.current.targetFps, 45)
document.visibilityState = 'hidden'
listeners.get('visibilitychange')()
assert.equal(budget.current.targetFps, 0)
assert.equal(budget.sample(1000), null)
budget.dispose()
console.log('RenderBudget: defaults, update and suspended PASS')
