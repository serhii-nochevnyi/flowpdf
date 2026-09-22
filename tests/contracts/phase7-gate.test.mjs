import assert from 'node:assert/strict'
import test from 'node:test'

import {
  assertSafeDiagnostic,
  assertValidationManifest,
  buildGateFingerprint,
  getPhaseSevenPlanStatus,
  parseValidationCommands,
  phaseSevenCoverageSummary,
  phaseSevenValidationTasks,
  readerBoundaryDiagnostics,
} from '../../scripts/check-phase7.mjs'

test('Phase 7 gate has an exact manifest and one terminal orchestration row', () => {
  const planned = parseValidationCommands()
  assert.equal(planned.length, 10)
  assertValidationManifest()
  assert.deepEqual(
    phaseSevenValidationTasks.map(({ id }) => id),
    planned.map(({ id }) => id),
  )
  assert.equal(phaseSevenValidationTasks.filter(({ mode }) => mode === 'terminal').length, 1)
})

test('Phase 7 records controlled local coverage separately from external evidence', () => {
  assert.match(phaseSevenCoverageSummary.requirements, /PDFI-01/)
  assert.match(phaseSevenCoverageSummary.local, /WASM/)
  assert.match(phaseSevenCoverageSummary.external, /unavailable/)
  const status = getPhaseSevenPlanStatus()
  assert.deepEqual(status.planIds, [1, 2, 3, 4])
  assert.deepEqual(status.missingPlanIds, [])
  assert.ok(status.openPlanIds.every((id) => Number.isInteger(id)))
})

test('Phase 7 source inspection rejects network, actions, and mutable reader handles', () => {
  const unsafe = new Map([
    [
      'web/src/pdf/pdf-reader-panel.tsx',
      `fetch('/telemetry'); window.open('https://example.test'); eval('x'); javascript:alert(1); new PdfReader()` ,
    ],
    ['crates/flow-core/src/pdf/reader.rs', 'pub struct PdfReadDiagnostic { bytes: Vec<u8> }'],
  ])
  const diagnostics = readerBoundaryDiagnostics(unsafe).join('\n')
  assert.match(diagnostics, /network access/i)
  assert.match(diagnostics, /external navigation/i)
  assert.match(diagnostics, /script execution/i)
  assert.match(diagnostics, /action callback/i)
  assert.match(diagnostics, /mutable reader handle|raw input/i)
})

test('Phase 7 current sources are clean and fingerprints carry no payloads', () => {
  assert.deepEqual(readerBoundaryDiagnostics(), [])
  const first = buildGateFingerprint()
  const second = buildGateFingerprint()
  assert.deepEqual(first, second)
  const serialized = JSON.stringify(first)
  assert.equal(serialized.includes('bytesHex'), false)
  assert.equal(serialized.includes('canonicalJson'), false)
  assert.equal(serialized.includes('javascript:'), false)
})

test('Phase 7 diagnostics are allowlisted without carrying PDF payloads', () => {
  assertSafeDiagnostic({ id: '07-04-01', status: 'pass', exitCode: 0, elapsedMilliseconds: 1 })
  assert.throws(
    () =>
      assertSafeDiagnostic({
        id: '07-04-01',
        status: 'pass',
        exitCode: 0,
        elapsedMilliseconds: 1,
        bytesHex: 'private-pdf',
      }),
    /unsafe gate diagnostic key/,
  )
})
