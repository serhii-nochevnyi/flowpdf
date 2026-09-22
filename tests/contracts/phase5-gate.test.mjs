import assert from 'node:assert/strict'
import test from 'node:test'

import {
  assertPhaseFiveClosed,
  assertSafeDiagnostic,
  assertValidationManifest,
  buildGateFingerprint,
  getPhaseFivePlanStatus,
  parseValidationCommands,
  phaseFiveCoverageSummary,
  phaseFiveValidationTasks,
} from '../../scripts/check-phase5.mjs'

test('Phase 5 gate has an exact manifest and a terminal orchestration row', () => {
  const planned = parseValidationCommands()
  assert.equal(planned.length, 12)
  assertValidationManifest()
  assert.deepEqual(
    phaseFiveValidationTasks.map(({ id }) => id),
    planned.map(({ id }) => id),
  )
  assert.equal(phaseFiveValidationTasks.at(-1)?.mode, 'terminal')
})

test('Phase 5 preflight exposes the current plan closure instead of inferring completion', () => {
  assert.equal(phaseFiveCoverageSummary.local, 'Rust/WASM/unit/browser/build/boundary')
  const status = getPhaseFivePlanStatus()
  assert.ok(status.planIds.length >= 25)
  assert.ok(status.openPlanIds.every((id) => Number.isInteger(id)))
  assert.deepEqual(status.missingPlanIds, [])
  assert.deepEqual(buildGateFingerprint(), buildGateFingerprint())
})

test('Phase 5 diagnostics are allowlisted and cannot carry source payloads', () => {
  assertSafeDiagnostic({ id: '05-01-01', status: 'pass', exitCode: 0, elapsedMilliseconds: 1 })
  assert.throws(
    () => assertSafeDiagnostic({ id: '05-01-01', status: 'pass', exitCode: 0, elapsedMilliseconds: 1, canonicalJson: 'payload' }),
    /unsafe gate diagnostic key/,
  )
})

test('Phase 5 closure gate rejects an unfinished plan inventory when present', () => {
  const { openPlanIds } = getPhaseFivePlanStatus()
  if (openPlanIds.length > 0) {
    assert.throws(() => assertPhaseFiveClosed(), /open plans/)
  } else {
    assertPhaseFiveClosed()
  }
})
