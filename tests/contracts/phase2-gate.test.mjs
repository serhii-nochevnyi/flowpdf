import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'

import {
  assertSafeDiagnostic,
  assertValidationManifest,
  buildGateFingerprint,
  parseValidationCommands,
  phaseTwoCoverageSummary,
  phaseTwoValidationTasks,
} from '../../scripts/check-phase2.mjs'

test('Phase 2 gate includes every validation task ID and exact command text', () => {
  const planned = parseValidationCommands()
  assert.equal(planned.length, 36)
  assertValidationManifest()
  assert.deepEqual(
    phaseTwoValidationTasks.map(({ id }) => id),
    planned.map(({ id }) => id),
  )
  assert.equal(phaseTwoValidationTasks.at(-1)?.commandText, 'npm run check:phase2:smoke && npm run check:phase2 && npm run check:phase2')
})

test('direct Vitest validation steps use the checked-in Playwright browser cache', () => {
  const browserTask = phaseTwoValidationTasks.find(({ id }) => id === '02-05-02')
  assert.equal(browserTask?.steps[1]?.env.PLAYWRIGHT_BROWSERS_PATH, `${process.cwd()}/work/playwright`)
})

test('Phase 2 gate preflight retains exact coverage and honest external status', () => {
  assert.deepEqual(phaseTwoCoverageSummary, {
    requirements: '7/7',
    edges: '33/33',
    ui: '108/108/0/0/0',
    prohibitions: '3/3 flagged',
    externalAt: 'unavailable/outstanding',
  })
  const fingerprint = buildGateFingerprint()
  assert.equal(fingerprint.uiEntryCount, 108)
  assert.equal(fingerprint.externalAt, 'unavailable/outstanding')
  assert.equal(fingerprint.scaleHashes.length, 2)
})

test('Phase 2 gate rejects missing or changed validation tasks', () => {
  const missing = phaseTwoValidationTasks.slice(0, -2).concat(phaseTwoValidationTasks.at(-1))
  assert.throws(() => assertValidationManifest(missing), /IDs\/commands drifted/)

  const changed = phaseTwoValidationTasks.map((task) => ({ ...task }))
  changed[0].commandText = `${changed[0].commandText} --changed`
  assert.throws(() => assertValidationManifest(changed), /IDs\/commands drifted/)
})

test('Phase 2 gate diagnostics are allowlisted and cannot carry content payloads', () => {
  assertSafeDiagnostic({ id: '02-18-01', status: 'pass', exitCode: 0, elapsedMilliseconds: 1 })
  assert.throws(
    () =>
      assertSafeDiagnostic({
        id: '02-18-01',
        status: 'pass',
        exitCode: 0,
        elapsedMilliseconds: 1,
        canonicalJson: 'Український authored payload',
      }),
    /unsafe gate diagnostic key/,
  )
  assert.throws(
    () => assertSafeDiagnostic({ id: '02-18-01', status: 'pass', exitCode: 0, elapsedMilliseconds: 1, code: 'FLOW_OK' }),
    /unsafe gate diagnostic key/,
  )
})

test('Phase 2 gate fingerprint is deterministic across preflight reads', () => {
  const first = buildGateFingerprint()
  const second = buildGateFingerprint()
  assert.deepEqual(first, second)
  assert.equal(JSON.stringify(first).includes('Український'), false)
  assert.equal(JSON.stringify(first).includes('👩‍💻'), false)
})

test('validation source keeps the terminal command as orchestration rather than recursion', () => {
  const validation = readFileSync(
    '.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-VALIDATION.md',
    'utf8',
  )
  assert.match(validation, /npm run check:phase2:smoke && npm run check:phase2 && npm run check:phase2/)
  assert.equal(phaseTwoValidationTasks.filter(({ mode }) => mode === 'terminal').length, 1)
})
