// Dependency-free checks using the repository's existing TypeScript compiler.
const assert = require('node:assert/strict')
const { execFileSync } = require('node:child_process')
const { mkdtempSync, rmSync } = require('node:fs')
const { tmpdir } = require('node:os')
const { join, resolve } = require('node:path')
const output = mkdtempSync(join(tmpdir(), 'lr8e-helpers-'))
const compiler = resolve('node_modules/typescript/bin/tsc')
async function main() {
  execFileSync(process.execPath, [compiler, '--strict', '--skipLibCheck', '--target', 'ES2022', '--module', 'commonjs', '--moduleResolution', 'node', '--outDir', output,
    'src/settings/operationalPolling.ts', 'src/settings/providerOperational.ts', 'src/settings/ratePolicyDraft.ts'], { stdio: 'inherit' })
  const { OperationalPoller } = require(join(output, 'operationalPolling.js'))
  const h = require(join(output, 'providerOperational.js'))
  const { policyDraft, editedPolicy } = require(join(output, 'ratePolicyDraft.js'))
  assert.equal(h.numberText(null), 'Desconhecido')
  assert.equal(h.numberText(0), '0')
  assert.equal(h.factText({ state: 'unknown' }, h.numberText), 'Desconhecido')
  assert.equal(h.scopeText({ kind: 'model', model: 'm' }), 'Model (m)')
  assert.equal(h.outcomeText({ kind: 'failed', code: 'Bearer private-error' }), 'Código não reconhecido')
  assert.equal(h.outcomeText({ kind: 'failed', code: 'toString' }), 'Código não reconhecido')
  const provider = { id: 'p', enabled: true, configured: true }
  const admission = { activeCalls: 0, maxConcurrency: 2, queueDepth: 0, queueCapacity: 64 }
  const rate = { constraints: [], persistenceFailed: false, contextGeneration: 0 }
  const resilience = { circuitState: 'closed', cooldownRemainingMs: 0 }
  assert.deepEqual(h.operationalConditions(provider, admission, rate, resilience), ['Disponível para tentativa'])
  assert(!h.operationalConditions(provider, admission, rate, { ...resilience, circuitState: 'open', openRemainingMs: 0 }).includes('Disponível para tentativa'))
  assert.equal(h.operationalConditions(provider, admission, rate, { ...resilience, cooldownRemainingMs: 10 })[0], 'Cooldown')
  assert(h.operationalConditions({ ...provider, enabled: false, configured: false }, admission, { ...rate, persistenceFailed: true }, { ...resilience, circuitState: 'half_open', halfOpenProbesActive: 1, halfOpenMaxProbes: 1 }).length >= 4)
  assert.deepEqual(h.operationalConditions(provider, admission, { ...rate, constraints: [{ scope: { kind: 'provider' }, dimension: 'tokens_per_day', effectiveRemaining: null }] }, resilience), ['Disponível para tentativa'])
  assert(h.operationalConditions(provider, { ...admission, activeCalls: 1, queueDepth: 64 }, rate, resilience).includes('Fila local sem vaga conhecida'))
  const modelGate = h.operationalConditions(provider, admission, { ...rate, constraints: [{ scope: { kind: 'model', model: 'm' }, dimension: 'requests_per_day', effectiveRemaining: 0, source: 'external_fact' }] }, resilience)
  assert.equal(modelGate[0], 'Disponível para tentativa · depende do modelo')
  assert(!modelGate.some(text => text.startsWith('Bloqueado')))
  const base = { limits: [{ scope: { kind: 'model', model: 'm' }, dimension: 'requests_per_minute', capacity: 5, window: { periodMs: 100, anchorUnixMs: 0 } }], dailyBudget: { anchorUnixMs: 0, maxRequests: 10, maxAccountedTokens: null } }
  const draft = policyDraft(base)
  draft.requests = '20'
  assert.deepEqual(editedPolicy(draft, base, base, false, true).limits, base.limits)
  const newer = { ...base, limits: [{ ...base.limits[0], capacity: 9 }] }
  assert.deepEqual(editedPolicy(draft, base, newer, false, true).limits, newer.limits)
  draft.limits[0].capacity = '8'
  assert.deepEqual(editedPolicy(draft, base, base, true, false).dailyBudget, base.dailyBudget)
  assert.throws(() => editedPolicy(draft, base, newer, true, false), /mudou/)
  for (const bad of ['', '-1', '1.5', '9007199254740992']) {
    const invalid = policyDraft(base); invalid.limits[0].capacity = bad
    assert.throws(() => editedPolicy(invalid, base, base, true, false), /inteiros/)
  }
  const empty = { limits: [], dailyBudget: null }
  const enabled = policyDraft(empty); enabled.budgetEnabled = true
  assert.throws(() => editedPolicy(enabled, empty, empty, false, true), /inteiros/)
  enabled.anchor = '0'
  assert.deepEqual(editedPolicy(enabled, empty, empty, false, true).dailyBudget, { anchorUnixMs: 0, maxRequests: null, maxAccountedTokens: null })
  const duplicate = policyDraft(base); duplicate.limits.push(duplicate.limits[0])
  assert.throws(() => editedPolicy(duplicate, base, base, true, false), /única/)
  let nextTimer = 0
  const timers = new Map()
  const originalSet = global.setTimeout, originalClear = global.clearTimeout
  global.setTimeout = (fn, ms) => { assert.equal(ms, 1000); timers.set(++nextTimer, fn); return nextTimer }
  global.clearTimeout = id => { timers.delete(id) }
  try {
    const reads = [], received = [], failures = [], busy = []
    const poller = new OperationalPoller(() => new Promise((resolve, reject) => reads.push({ resolve, reject })), v => received.push(v), () => failures.push(true), v => busy.push(v))
    const flush = async () => { for (let n = 0; n < 12; n++) await Promise.resolve() }
    poller.start(true)
    assert.equal(reads.length, 1)
    assert.equal(timers.size, 0)
    const refreshed = poller.refresh() // manual update during initial pending read
    assert.equal(reads.length, 1)
    reads[0].resolve('old')
    await flush()
    assert.equal(reads.length, 2)
    reads[1].resolve('new')
    assert.equal(await refreshed, true)
    assert.equal(received.at(-1), 'new')
    assert.equal(timers.size, 1)
    poller.setVisible(false)
    assert.equal(timers.size, 0)
    assert.equal(await poller.refresh(), false)
    assert.equal(reads.length, 2)
    poller.setVisible(true)
    assert.equal(reads.length, 3)
    poller.setVisible(false)
    reads[2].resolve('hidden response')
    await flush()
    assert(!received.includes('hidden response'))
    assert.equal(timers.size, 0)
    poller.start(true)
    assert.equal(reads.length, 4)
    poller.stop(); poller.start(true) // StrictMode cleanup/setup before IPC completes
    assert.equal(reads.length, 4)
    reads[3].resolve('disposed epoch')
    await flush()
    assert(!received.includes('disposed epoch'))
    assert.equal(reads.length, 5)
    reads[4].reject(new Error('private remote body'))
    await flush()
    assert.deepEqual(failures, [true])
    assert.equal(received.at(-1), 'new') // last good snapshot is preserved
    assert.equal(timers.size, 1)
    const fn = [...timers.values()][0]; timers.clear(); fn()
    assert.equal(reads.length, 6)
    poller.stop()
    reads[5].resolve('unmounted response')
    await flush()
    assert(!received.includes('unmounted response'))
    assert.equal(timers.size, 0)
    assert.equal(busy.at(-1), true) // no state writes into unmounted component
  } finally { global.setTimeout = originalSet; global.clearTimeout = originalClear }
  console.log('LR-8E helpers: PASS (unknown, scopes, outcomes, policy merge/validation, polling lifecycle/single-flight/stale/error).')
}
main().catch(error => { console.error(error); process.exitCode = 1 }).finally(() => rmSync(output, { recursive: true, force: true }))
