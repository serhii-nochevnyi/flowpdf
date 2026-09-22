import assert from 'node:assert/strict'
import test from 'node:test'

import {
  assertSafeDiagnostic,
  assertValidationManifest,
  buildGateFingerprint,
  getPhaseEightPlanStatus,
  parseValidationCommands,
  phaseEightCoverageSummary,
  phaseEightValidationTasks,
  reconstructionBoundaryDiagnostics,
} from '../../scripts/check-phase8.mjs'

test('Phase 8 gate has the exact ten-row manifest and one terminal orchestration row', () => {
  const planned = parseValidationCommands()
  assert.equal(planned.length, 10)
  assertValidationManifest()
  assert.deepEqual(
    phaseEightValidationTasks.map(({ id }) => id),
    planned.map(({ id }) => id),
  )
  assert.equal(phaseEightValidationTasks.filter(({ mode }) => mode === 'terminal').length, 1)
})

test('Phase 8 records bounded local coverage separately from external evidence', () => {
  assert.match(phaseEightCoverageSummary.requirements, /PDFI-04/)
  assert.match(phaseEightCoverageSummary.local, /immutable source store/i)
  assert.match(phaseEightCoverageSummary.external, /unavailable/)
  const status = getPhaseEightPlanStatus()
  assert.deepEqual(status.planIds, [1, 2, 3, 4])
  assert.deepEqual(status.missingPlanIds, [])
  assert.ok(status.openPlanIds.every((id) => Number.isInteger(id)))
})

test('Phase 8 source inspection rejects network, actions, mutable handles, payload diagnostics, and exact claims', () => {
  const unsafe = new Map([
    [
      'web/src/pdf/pdf-reconstruction-panel.tsx',
      `fetch('/telemetry'); window.open('https://example.test'); eval('x'); javascript:alert(1); new PdfReconstruction(); exact reconstruction is guaranteed`,
    ],
    ['crates/flow-core/src/pdf/reconstruction.rs', 'pub struct PdfReconstructionDiagnostic { bytes: Vec<u8> }'],
    ['web/src/pdf/pdf-reconstruction-protocol.ts', 'interface PdfReconstructionDiagnosticDto { text: string }'],
    ['web/persistence/pdf-source-store.ts', 'store.delete(sourceHash); objectStore.put(candidate)'],
  ])
  const diagnostics = reconstructionBoundaryDiagnostics(unsafe).join('\n')
  assert.match(diagnostics, /network access/i)
  assert.match(diagnostics, /external navigation/i)
  assert.match(diagnostics, /script execution/i)
  assert.match(diagnostics, /external action/i)
  assert.match(diagnostics, /mutable reconstruction handle/i)
  assert.match(diagnostics, /positive exact claim/i)
  assert.match(diagnostics, /raw input\/OCR payload|source\/OCR payload/i)
  assert.match(diagnostics, /overwrite\/delete/i)
})

test('Phase 8 current sources are clean and fingerprints carry no payloads or external actions', () => {
  assert.deepEqual(reconstructionBoundaryDiagnostics(), [])
  const first = buildGateFingerprint()
  const second = buildGateFingerprint()
  assert.deepEqual(first, second)
  const serialized = JSON.stringify(first)
  assert.equal(serialized.includes('bytesHex'), false)
  assert.equal(serialized.includes('imageDataHex'), false)
  assert.equal(serialized.includes('canonicalJson'), false)
  assert.equal(serialized.includes('javascript:'), false)
  assert.equal(serialized.includes('exact reconstruction is guaranteed'), false)
})

test('Phase 8 diagnostics are allowlisted without carrying source or OCR payloads', () => {
  assertSafeDiagnostic({ id: '08-04-01', status: 'pass', exitCode: 0, elapsedMilliseconds: 1 })
  assert.throws(
    () =>
      assertSafeDiagnostic({
        id: '08-04-01',
        status: 'pass',
        exitCode: 0,
        elapsedMilliseconds: 1,
        bytesHex: 'private-pdf',
      }),
    /unsafe gate diagnostic key/,
  )
})
