import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

import {
  assertSafeDiagnostic,
  assertValidationManifest,
  buildGateFingerprint,
  parseValidationCommands,
  phaseThreeCoverageSummary,
  selectPhaseThreeSteps,
  phaseThreeValidationTasks,
} from '../../scripts/check-phase3.mjs'

test('Phase 3 gate includes every exact validation task and command', () => {
  const planned = parseValidationCommands()
  assert.equal(planned.length, 17)
  assertValidationManifest()
  assert.deepEqual(
    phaseThreeValidationTasks.map(({ id }) => id),
    planned.map(({ id }) => id),
  )
  assert.equal(
    phaseThreeValidationTasks.at(-1)?.commandText,
    'npm run check:phase3:smoke && npm run check:phase3 && npm run check:phase3',
  )
})

test('Phase 3 browser rows use the checked-in Playwright cache and no dependency mutation', () => {
  const browserTask = phaseThreeValidationTasks.find(({ id }) => id === '03-06-01')
  assert.equal(browserTask?.steps[1]?.env.PLAYWRIGHT_BROWSERS_PATH, `${process.cwd()}/work/playwright`)
  const manifest = readFileSync(
    '.planning/phases/FLOWPDF-03-deterministic-reflow-and-pagination/03-VALIDATION.md',
    'utf8',
  )
  assert.doesNotMatch(manifest, /npm\s+(?:ci|install)|cargo\s+update/i)
  assert.doesNotMatch(manifest, /Phase 4|Phase 5|Phase 6|Phase 7|Phase 8|Phase 9/)
})

test('Phase 3 preflight reports complete local LAYO coverage and honest inherited AT status', () => {
  assert.deepEqual(phaseThreeCoverageSummary, {
    requirements: '8/8 local',
    rust: 'deterministic/pinned',
    worker: 'revision-safe',
    browser: 'Chromium/pinned-cache',
    phase2ExternalAt: 'unavailable/outstanding',
  })
  const fingerprint = buildGateFingerprint()
  assert.equal(fingerprint.phase2ExternalAt, 'unavailable/outstanding')
  assert.equal(fingerprint.validationTaskIds.length, 17)
  assert.equal(Object.keys(fingerprint.artifactHashes).length, 19)
  assert.deepEqual(fingerprint, buildGateFingerprint())
  assert.equal(JSON.stringify(fingerprint).includes('Український'), false)
  assert.equal(JSON.stringify(fingerprint).includes('Boundary secret'), false)
})

test('Phase 3 manifest rejects missing, reordered, or changed task rows', () => {
  const missing = phaseThreeValidationTasks.slice(0, -2).concat(phaseThreeValidationTasks.at(-1))
  assert.throws(() => assertValidationManifest(missing), /IDs\/commands drifted/)

  const changed = phaseThreeValidationTasks.map((task) => ({ ...task }))
  changed[0].commandText = `${changed[0].commandText} --changed`
  assert.throws(() => assertValidationManifest(changed), /IDs\/commands drifted/)

  const reordered = [...phaseThreeValidationTasks]
  const first = reordered.shift()
  if (first !== undefined) reordered.splice(1, 0, first)
  assert.throws(() => assertValidationManifest(reordered), /IDs\/commands drifted/)
})

test('Phase 3 diagnostics are allowlisted and cannot carry layout/source payloads', () => {
  assertSafeDiagnostic({ id: '03-05-03', status: 'pass', exitCode: 0, elapsedMilliseconds: 1 })
  assert.throws(
    () =>
      assertSafeDiagnostic({
        id: '03-05-03',
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
        id: '03-05-03',
        status: 'pass',
        exitCode: 0,
        elapsedMilliseconds: 1,
        fontBytes: [1, 2, 3],
      }),
    /unsafe gate diagnostic key/,
  )
  assert.throws(
    () => assertSafeDiagnostic({ id: '03-05-03', status: 'pass', exitCode: 0, elapsedMilliseconds: 1, code: 'FLOW_OK' }),
    /unsafe gate diagnostic key/,
  )
})

test('release-child mode does not rerun the inherited Phase 1 step', () => {
  const task = phaseThreeValidationTasks.find(({ id }) => id === '03-06-06')
  assert.equal(selectPhaseThreeSteps(task, { includePhaseOne: true }).length, 1)
  assert.equal(selectPhaseThreeSteps(task, { includePhaseOne: false }).length, 0)
})
