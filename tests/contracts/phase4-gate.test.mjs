import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

import {
  assertNoFutureScope,
  assertReferenceDiagnostic,
  assertRevisionSafeWorkerContract,
  assertSafeDiagnostic,
  assertValidationManifest,
  buildGateFingerprint,
  parseReferenceCommands,
  parseValidationCommands,
  phaseFourCoverageSummary,
  phaseFourReferenceTasks,
  phaseFourValidationTasks,
} from '../../scripts/check-phase4.mjs'

test('Phase 4 gate includes every exact local and reference validation row', () => {
  const planned = parseValidationCommands()
  const plannedReferences = parseReferenceCommands()
  assert.equal(planned.length, 12)
  assert.equal(plannedReferences.length, 4)
  assertValidationManifest()
  assert.deepEqual(
    phaseFourValidationTasks.map(({ id }) => id),
    planned.map(({ id }) => id),
  )
  assert.deepEqual(
    phaseFourReferenceTasks.map(({ id }) => id),
    plannedReferences.map(({ id }) => id),
  )
  assert.equal(
    phaseFourValidationTasks.at(-1)?.commandText,
    'npm run check:phase4:smoke && npm run check:phase4 && npm run check:phase4',
  )
})

test('Phase 4 rows do not mutate dependencies and browser rows use the pinned cache', () => {
  const browserTask = phaseFourValidationTasks.find(({ id }) => id === '04-05-03')
  assert.equal(browserTask?.steps[1]?.env.PLAYWRIGHT_BROWSERS_PATH, `${process.cwd()}/work/playwright`)
  const manifest = readFileSync(
    '.planning/phases/FLOWPDF-04-owned-pdf-preview-and-export/04-VALIDATION.md',
    'utf8',
  )
  assert.doesNotMatch(manifest, /npm\s+(?:ci|install)|cargo\s+update/i)
  assert.doesNotMatch(manifest, /Phase 5|Phase 6|Phase 7|Phase 8|Phase 9/)
  assert.doesNotMatch(manifest.split('## Evidence boundary')[0], /AcroForm|OCR|voice|PDF\/A|PDF\/UA/i)
})

test('Phase 4 preflight reports complete mapping and deterministic safe fingerprints', () => {
  assert.deepEqual(phaseFourCoverageSummary, {
    requirements: '9/9 mapped',
    local: 'COS/resources/provenance/WASM/worker/preview',
    regressions: 'Phase 1/2/3 local gates retained',
    reference: 'pass/fail/unavailable; no tool or fixture is not a pass',
    phase2ExternalAt: 'unavailable/outstanding',
  })
  const first = buildGateFingerprint()
  const second = buildGateFingerprint()
  assert.deepEqual(first, second)
  assert.equal(first.validationTaskIds.length, 12)
  assert.equal(first.referenceTaskIds.length, 4)
  assert.ok(Object.keys(first.artifactHashes).length >= 20)
  assert.equal(first.phase2ExternalAt, 'unavailable/outstanding')
  assert.equal(JSON.stringify(first).includes('Український'), false)
  assert.equal(JSON.stringify(first).includes('Boundary secret'), false)
})

test('Phase 4 manifest rejects missing, reordered, changed, and future-scope rows', () => {
  const missing = phaseFourValidationTasks.slice(0, -2).concat(phaseFourValidationTasks.at(-1))
  assert.throws(() => assertValidationManifest(missing), /IDs\/commands drifted/)

  const changed = phaseFourValidationTasks.map((task) => ({ ...task }))
  changed[0].commandText = `${changed[0].commandText} --changed`
  assert.throws(() => assertValidationManifest(changed), /IDs\/commands drifted/)

  const reordered = [...phaseFourValidationTasks]
  const first = reordered.shift()
  if (first !== undefined) reordered.splice(1, 0, first)
  assert.throws(() => assertValidationManifest(reordered), /IDs\/commands drifted/)

  assert.throws(() => assertNoFutureScope('| 04-06-01 | Phase 5 forms | `npm test` |'), /future or deferred/)
})

test('Phase 4 diagnostics are allowlisted and reference absence cannot pass', () => {
  assertSafeDiagnostic({ id: '04-05-02', status: 'pass', exitCode: 0, elapsedMilliseconds: 1 })
  assertSafeDiagnostic({ id: '04-06-R1', status: 'unavailable', exitCode: 127, elapsedMilliseconds: 1 })
  assertReferenceDiagnostic(
    { id: '04-06-R1', status: 'unavailable', exitCode: 127, elapsedMilliseconds: 1 },
    { toolAvailable: false, artifactAvailable: false },
  )
  assert.throws(
    () =>
      assertReferenceDiagnostic(
        { id: '04-06-R1', status: 'pass', exitCode: 0, elapsedMilliseconds: 1 },
        { toolAvailable: false, artifactAvailable: true },
      ),
    /cannot be recorded as pass/,
  )
  assert.throws(
    () =>
      assertSafeDiagnostic({
        id: '04-05-02',
        status: 'pass',
        exitCode: 0,
        elapsedMilliseconds: 1,
        canonicalJson: 'authored source payload',
      }),
    /unsafe gate diagnostic key/,
  )
  assert.throws(
    () =>
      assertSafeDiagnostic({
        id: '04-05-02',
        status: 'pass',
        exitCode: 0,
        elapsedMilliseconds: 1,
        pdfBytes: [1, 2, 3],
      }),
    /unsafe gate diagnostic key/,
  )
})

test('Phase 4 smoke contract requires revision-safe stale-result guards', () => {
  assertRevisionSafeWorkerContract()
  assert.throws(
    () => assertRevisionSafeWorkerContract('validatePdfExportResult(request, result)'),
    /stale-result guard missing/,
  )
})
