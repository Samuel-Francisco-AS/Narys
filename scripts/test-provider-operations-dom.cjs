// Static React rendering of the real card with synthetic DTOs. No app startup,
// Tauri invocation, commercial HTTP, credentials or browser test dependency.
const assert = require('node:assert/strict')
const { execFileSync } = require('node:child_process')
const { mkdtempSync, rmSync, writeFileSync, appendFileSync, symlinkSync, readFileSync } = require('node:fs')
const { tmpdir } = require('node:os')
const { join, resolve } = require('node:path')
const temporary = mkdtempSync(join(tmpdir(), 'lr8e-dom-'))
try {
  execFileSync(process.execPath, [resolve('node_modules/typescript/bin/tsc'), '--strict', '--skipLibCheck', '--target', 'ES2022', '--module', 'commonjs', '--moduleResolution', 'node', '--jsx', 'react-jsx', '--outDir', temporary, 'src/settings/ProviderOperationsPanel.tsx'], { stdio: 'inherit' })
  symlinkSync(resolve('node_modules'), join(temporary, 'node_modules'))
  writeFileSync(join(temporary, 'settings.css'), '')
  require.extensions['.css'] = () => {}
  // Expose the private component only in temporary emitted JS for this harness.
  appendFileSync(join(temporary, 'ProviderOperationsPanel.js'), '\nmodule.exports.__qaCard = ProviderCard; module.exports.__qaControls = OperationsRefreshControls;\n')
  const { __qaCard: Card, __qaControls: Controls } = require(join(temporary, 'ProviderOperationsPanel.js'))
  const React = require('react'), { renderToStaticMarkup } = require('react-dom/server')
  const unknown = { state: 'unknown' }
  const known = value => ({ state: 'known', value, provenance: 'provider_header', observedAtUnixMs: 1000 })
  const provider = { id: 'groq', displayName: 'Groq', configured: true, enabled: true, privateKey: 'sk-qa-secret', accountId: 'private-account-marker', prompt: 'private prompt', response: 'private response', reasoning: 'private reasoning', authorization: 'Bearer secret' }
  const usage = Object.fromEntries(['requests', 'input_tokens', 'output_tokens', 'total_tokens', 'thought_tokens'].map(k => [k, { observed: k === 'requests' ? known(10) : k === 'input_tokens' ? known(1234) : unknown, reportingRequests: k === 'requests' ? 10 : k === 'input_tokens' ? 5 : 0, saturated: k === 'input_tokens' }]))
  const dimensions = Object.fromEntries(['requests_per_minute', 'tokens_per_minute', 'requests_per_day', 'tokens_per_day', 'concurrency'].map(k => [k, { limit: k === 'requests_per_day' ? known(14400) : unknown, remaining: unknown, reset: unknown }]))
  const telemetry = { providerId: 'groq', usage, quotas: [{ scope: { kind: 'provider' }, dimensions }, { scope: { kind: 'model', model: 'model-a' }, dimensions }], retryHint: known({ kind: 'delay_ms', value: 9999 }), lastOutcome: known({ kind: 'failed', code: 'private-remote-output' }), updatedAgeMs: 200 }
  const admission = { activeCalls: 2, maxConcurrency: 2, queueDepth: 1, queueCapacity: 64, queuedByClass: { foreground_interactive: 1, foreground_task: 0, background: 0 }, totalAdmissions: 2, totalWaited: 1, queueDelaySamples: 0, queueDelayTotalMs: 0, queueDelayRecentMs: null, queueFullCount: 0, queueTimeoutCount: 0, countersSaturated: false }
  const rate = { providerId: 'groq', contextGeneration: 0, constraints: [{ scope: { kind: 'model', model: 'model-a' }, dimension: 'tokens_per_day', source: 'local_policy', provenance: 'user_configuration', capacity: 100, consumed: 50, reserved: 0, effectiveRemaining: null, resetUnixMs: null, resetInMs: null, unaccountedTokenCalls: 1, saturated: true, external: null }], policy: { limits: [], dailyBudget: null }, pendingReservations: 1, localBlocks: 2, persistenceFailed: true, saturated: true }
  const resilience = { circuitState: 'half_open', cooldownRemainingMs: 50, halfOpenProbesActive: 1, halfOpenMaxProbes: 1, consecutiveEligibleFailures: 3, configuredThreshold: 3, openRemainingMs: 0, transitionCount: 2, lastTransitionReason: 'probe_timeout', breakerOpenCount: 1, halfOpenCount: 1, recoveryCount: 0, saturated: false }
  const html = renderToStaticMarkup(React.createElement(Card, { provider, telemetry, admission, rate, resilience, refresh: async () => false }))
  for (const marker of ['sk-qa-secret', 'Bearer secret', 'private-account-marker', 'private prompt', 'private response', 'private reasoning', 'private-remote-output', '<canvas', '<script', 'aria-live=']) assert(!html.includes(marker), marker)
  for (const required of ['Desconhecido', 'Medição parcial', 'Model (model-a)', 'reportaram', 'Estado local de rate não pôde ser persistido', 'Accounting unresolved', 'Último Retry-After observado', 'Cooldown operacional restante', 'Sem amostras', 'Configuração local', 'sem contrato de preço configurado', 'Half-open / sondagem', 'Código não reconhecido', '<caption>', 'scope="col"', 'disabled=""']) assert(html.includes(required), required)
  assert(!html.includes('Saudável'))
  const capturedAtUnixMs = 1_791_000_000_000
  const controls = (manualRefreshing, captured = capturedAtUnixMs) => renderToStaticMarkup(React.createElement(Controls, {
    snapshot: { capturedAtUnixMs: captured }, visible: true, manualRefreshing, refreshManual: () => {},
  }))
  const automatic = controls(false), nextAutomatic = controls(false, capturedAtUnixMs + 1000)
  // Only the optional diagnostic title changes on the automatic capture.
  const surface = markup => markup.replace(/ title="[^"]*"/g, '')
  assert.equal(surface(automatic), surface(nextAutomatic))
  assert(automatic.includes('Atualizar agora') && !automatic.includes('Atualizando') && !automatic.includes('disabled='))
  assert(!surface(automatic).includes(String(capturedAtUnixMs)))
  assert(!surface(automatic).includes('Unix ms UTC'))
  assert(surface(automatic).includes('Última captura recebida'))
  const manual = controls(true)
  assert(manual.includes('Atualizando…') && manual.includes('disabled=""'))
  for (const markup of [automatic, nextAutomatic, manual, controls(false, null), controls(false, -1)]) {
    assert(!markup.includes('aria-live=') && !markup.includes('role="status"'))
  }
  assert(controls(false, null).includes('Desconhecido') && !controls(false, -1).includes('title='))
  const hidden = renderToStaticMarkup(React.createElement(Controls, { snapshot: null, visible: false, manualRefreshing: false, refreshManual: () => {} }))
  assert(hidden.includes('disabled=""') && hidden.includes('Aguardando snapshot'))
  const preview = '<!doctype html><html lang="pt-BR"><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1"><style>*{box-sizing:border-box}body{margin:0}' + readFileSync('src/settings/settings.css', 'utf8') + '</style><main class="settings-page"><h1>QA sintética · LR-8E</h1><h2>Operação dos providers</h2>' + html.replaceAll('<details ', '<details open ') + '</main></html>'
  writeFileSync('/tmp/lr8e-panel-qa.html', preview)
  console.log('LR-8E DOM: PASS (markers privados ausentes, unknown/parcial/scopes, warnings, tabelas semânticas, toolbar automática estável, feedback somente manual, timestamp discreto, sem canvas/script/aria-live periódico).')
} finally { rmSync(temporary, { recursive: true, force: true }) }
