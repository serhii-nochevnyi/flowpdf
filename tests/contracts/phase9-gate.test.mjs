import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { test } from 'node:test'
import { isProductInput, runTasks, safeDiagnostic } from '../../scripts/check-phase9.mjs'
import { tasks, assertManifest } from '../../scripts/phase9-validation-manifest.mjs'

test('inventory covers each planned task once and runs the existing note tracer', () => {
  const counts = [2, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 2, 2, 2]
  const expected = counts.flatMap((count, plan) => Array.from({ length: count },
    (_, index) => `09-${String(plan + 1).padStart(2, '0')}-0${index + 1}`))
  assert.deepEqual(tasks.flatMap(({ covers }) => covers).sort(), expected)
  assert.deepEqual(tasks.find(({ id }) => id === '09-03-02').argv,
    ['node', 'scripts/check-native-note-tracer.mjs', '--run'])
  assert.equal(tasks.at(-1).lane, 'terminal')
  const inventory = createHash('sha256').update(JSON.stringify(tasks)).digest('hex')
  assert.equal(inventory, '738751c34a731de2bea1bee50c4fb5ccf7e5cbc248f0eb15fe71da91e1523d58')
  assert.throws(() => assertManifest(tasks.slice(1)))
  assert.throws(() => assertManifest(tasks.toReversed()))
  assert.throws(() => assertManifest(tasks.map((task) => ({ ...task, argv: [] }))))
  assert.throws(() => assertManifest(tasks.map((task) => ({ ...task, argv: ['npm', 'install'] }))))
})

test('only declared product inputs participate in the fingerprint', () => {
  for (const path of ['.planning/09-VALIDATION.md', '.shipyard/audit.json', 'scripts/AGENTS.md',
    'web/CLAUDE.md', 'docs/audits/phase9.md', 'audit/phase9.json', 'artifacts/audits/phase9.json',
    'scripts/.agents/review.json', 'scripts/.planning/review.json', 'scripts/.shipyard-review.json',
    'artifacts/phase9/local-validation.json', 'web/generated/test.js']) {
    assert.equal(isProductInput(path), false, path)
  }
  for (const path of ['crates/flow-core/src/pdf/native/graph.rs', 'web/src/pdf/pdf-native-edit-worker.ts',
    'tests/contracts/phase9-gate.test.mjs', 'fixtures/pdf-native/redaction.pdf', 'package.json',
    'crates/flow-core/src/audit/mod.rs', 'crates/flow-core/tests/audit_redaction.rs',
    '.node-version', '.github/workflows/pdf-native-core.yml']) {
    assert.equal(isProductInput(path), true, path)
  }
  assert.ok(tasks.every(({ argv }) => argv.every((value) => !/\.planning|\.shipyard|install$/.test(value) || value === '--no-install')))
})

test('execution fails fast and reports bounded diagnostic fields', () => {
  const calls = []
  const result = runTasks('local', (argv) => {
    calls.push(argv)
    return { status: 4, stderr: 'private payload' }
  })
  assert.equal(calls.length, 1)
  assert.equal(result.exitCode, 4)
  assert.equal(result.tasks[0].status, 'fail')
  assert.ok(!JSON.stringify(result).includes('private payload'))
  assert.throws(() => runTasks('terminal', () => assert.fail()))
  assert.throws(() => safeDiagnostic({ ...result.tasks[0], stdout: 'private payload' }))
})
