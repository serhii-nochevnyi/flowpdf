import assert from 'node:assert/strict'
import test from 'node:test'

import {
  assertSafeDiagnostic,
  assertValidationManifest,
  buildGateFingerprint,
  getPhaseSixPlanStatus,
  parseValidationCommands,
  phaseSixCoverageSummary,
  phaseSixValidationTasks,
  voicePrivacyDiagnostics,
} from '../../scripts/check-phase6.mjs'

test('Phase 6 gate has an exact manifest and a terminal orchestration row', () => {
  const planned = parseValidationCommands()
  assert.equal(planned.length, 15)
  assertValidationManifest()
  assert.deepEqual(
    phaseSixValidationTasks.map(({ id }) => id),
    planned.map(({ id }) => id),
  )
  assert.equal(phaseSixValidationTasks.at(-1)?.mode, 'terminal')
})

test('Phase 6 gate records local coverage and external speech/AT limits separately', () => {
  assert.equal(phaseSixCoverageSummary.local, 'Rust/WASM/unit/browser/build/boundary/privacy')
  assert.match(phaseSixCoverageSummary.external, /unavailable/)
  const status = getPhaseSixPlanStatus()
  assert.deepEqual(status.planIds, [1, 2, 3, 4, 5, 6])
  assert.deepEqual(status.missingPlanIds, [])
  assert.ok(status.openPlanIds.every((id) => Number.isInteger(id)))
})

test('Phase 6 privacy source inspection rejects storage, network, analytics, and audio fixtures', () => {
  const fixture = {
    typescript: new Map([
      [
        'web/src/voice/unsafe.ts',
        `
          export function unsafe() {
            localStorage.setItem('voice', 'raw transcript')
            fetch('/analytics')
            navigator.mediaDevices.getUserMedia({ audio: true })
            analytics.track('voice')
          }
        `,
      ],
    ]),
  }
  const diagnostics = voicePrivacyDiagnostics(fixture).join('\n')
  assert.match(diagnostics, /browser storage/i)
  assert.match(diagnostics, /network\/telemetry/i)
  assert.match(diagnostics, /independent audio capture/i)
})

test('Phase 6 current voice sources are privacy-clean and fingerprints contain no payloads', () => {
  assert.deepEqual(voicePrivacyDiagnostics(), [])
  const first = buildGateFingerprint()
  const second = buildGateFingerprint()
  assert.deepEqual(first, second)
  assert.equal(JSON.stringify(first).includes('transcript'), false)
  assert.equal(JSON.stringify(first).includes('canonicalJson'), false)
})

test('Phase 6 diagnostics are allowlisted and cannot carry transcript or canonical payloads', () => {
  assertSafeDiagnostic({ id: '06-06-01', status: 'pass', exitCode: 0, elapsedMilliseconds: 1 })
  assert.throws(
    () =>
      assertSafeDiagnostic({
        id: '06-06-01',
        status: 'pass',
        exitCode: 0,
        elapsedMilliseconds: 1,
        transcript: 'private speech',
      }),
    /unsafe gate diagnostic key/,
  )
  assert.throws(
    () =>
      assertSafeDiagnostic({
        id: '06-06-01',
        status: 'pass',
        exitCode: 0,
        elapsedMilliseconds: 1,
        canonicalJson: '{"private":"source"}',
      }),
    /unsafe gate diagnostic key/,
  )
})
