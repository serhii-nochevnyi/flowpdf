import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { test } from 'node:test'
import { isProductInput, runTasks, safeDiagnostic } from '../../scripts/check-phase9.mjs'
import { tasks, assertManifest } from '../../scripts/phase9-validation-manifest.mjs'

test('inventory covers each planned task once and runs the existing note tracer', () => {
  const counts = [2, 3, 3, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 3, 2, 2, 2]
  const expected = counts.flatMap((count, plan) => Array.from({ length: count },
    (_, index) => `09-${String(plan + 1).padStart(2, '0')}-0${index + 1}`))
  assert.deepEqual(tasks.flatMap(({ covers }) => covers).sort(), [...expected, '09-27-01', '09-60-01', '09-77-01'])
  assert.deepEqual(tasks.find(({ id }) => id === '09-03-02').argv,
    ['node', 'scripts/check-native-note-tracer.mjs', '--run'])
  assert.equal(tasks.at(-1).lane, 'terminal')
  const inventory = createHash('sha256').update(JSON.stringify(tasks)).digest('hex')
  assert.equal(inventory, 'ec51979a491a681962f750d3ec15a88a2122d840169a27384e13c14653497f37')
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

test('local execution builds WASM in a clean checkout and runs the reader, checksum, and fixture suites', (t) => {
  const cwd = mkdtempSync(join(tmpdir(), 'phase9-gate-'))
  t.after(() => rmSync(cwd, { recursive: true, force: true }))
  const calls = []
  const result = runTasks('local', (argv, options) => {
    calls.push(argv)
    if (calls.length === 1) {
      assert.deepEqual(argv, ['npm', 'run', 'build:wasm'])
      const generated = join(cwd, 'web/generated')
      assert.equal(existsSync(generated), false)
      mkdirSync(generated, { recursive: true })
      for (const name of ['flow_wasm.js', 'flow_wasm.d.ts', 'flow_wasm_bg.wasm']) {
        writeFileSync(join(generated, name), 'test binding')
      }
    }
    if (argv.includes('tests/contracts/pdf-native-fixtures.test.mjs')) {
      assert.equal(options.cwd, cwd)
      assert.equal(options.env.PHASE9_REQUIRE_WASM, '1')
    }
    return { status: 0 }
  }, cwd)
  assert.equal(result.status, 'pass')
  assert.deepEqual(calls.filter((argv) => argv.includes('scripts/verify-wasm-bindgen-tool.test.mjs')),
    [['node', '--test', 'scripts/verify-wasm-bindgen-tool.test.mjs']])
  assert.deepEqual(calls.filter((argv) => argv.includes('flow-wasm') && argv.includes('pdf_reader')),
    [['cargo', 'test', '--offline', '--locked', '-p', 'flow-wasm', '--test', 'pdf_native_edit', '--test', 'pdf_reader', '--', '--nocapture']])
  assert.ok(calls.some((argv) => argv.includes('tests/contracts/pdf-native-fixtures.test.mjs')))
})

test('local execution stops when the WASM build fails or a required output is missing', (t) => {
  const cwd = mkdtempSync(join(tmpdir(), 'phase9-gate-'))
  t.after(() => rmSync(cwd, { recursive: true, force: true }))
  const generated = join(cwd, 'web/generated')
  mkdirSync(generated, { recursive: true })
  const outputs = ['flow_wasm.js', 'flow_wasm.d.ts', 'flow_wasm_bg.wasm']
  const cases = [
    { buildStatus: 1 },
    ...outputs.map((missing) => ({ buildStatus: 0, missing })),
    { buildStatus: 0, empty: 'flow_wasm_bg.wasm' },
  ]
  for (const { buildStatus, missing, empty } of cases) {
    for (const name of outputs) {
      const path = join(generated, name)
      if (name === missing) rmSync(path, { force: true })
      else writeFileSync(path, name === empty ? '' : 'test binding')
    }
    const calls = []
    const result = runTasks('local', (argv) => {
      calls.push(argv)
      return { status: buildStatus }
    }, cwd)
    assert.deepEqual(calls, [['npm', 'run', 'build:wasm']])
    assert.equal(result.status, 'unavailable')
    assert.equal(result.exitCode, 2)
    assert.equal(result.missing, 'generated-wasm')
    assert.equal(result.tasks[0].status, 'unavailable')
  }
})

test('execution fails fast and reports bounded diagnostic fields', () => {
  const calls = []
  const result = runTasks('smoke', (argv) => {
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
