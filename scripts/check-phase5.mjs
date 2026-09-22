#!/usr/bin/env node

import { readFileSync, readdirSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { delimiter, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { assertSupportedNodeVersion } from './node-version.mjs'
import { runBoundedStep, writeFailureDetails } from './phase-gate-runner.mjs'

assertSupportedNodeVersion()

export const projectRoot = resolve(import.meta.dirname, '..')
const cargoHome = resolve(projectRoot, 'work/toolchains/cargo')
const rustupHome = resolve(projectRoot, 'work/toolchains/rustup')
const cargoBinary = resolve(cargoHome, 'bin/cargo')
const npmBinary = process.platform === 'win32' ? 'npm.cmd' : 'npm'
const npxBinary = process.platform === 'win32' ? 'npx.cmd' : 'npx'
const rustEnvironment = Object.freeze({
  ...process.env,
  CARGO_HOME: cargoHome,
  RUSTUP_HOME: rustupHome,
  PATH: `${dirname(cargoBinary)}${delimiter}${process.env.PATH ?? ''}`,
})
const browserEnvironment = Object.freeze({
  ...process.env,
  PLAYWRIGHT_BROWSERS_PATH: resolve(projectRoot, 'work/playwright'),
})
const phaseDirectory = resolve(projectRoot, '.planning/phases/FLOWPDF-05-semantic-fillable-forms')
const phaseValidationPath = resolve(phaseDirectory, '05-VALIDATION.md')
const phaseClosurePath = resolve(phaseDirectory, '05-CLOSURE.md')

export const phaseFiveCoverageSummary = Object.freeze({
  requirements: 'local form validation/session/projection/WASM/UI/PDF seams',
  local: 'Rust/WASM/unit/browser/build/boundary',
  external: 'explicit release input; unavailable is not pass',
})

const fingerprintFiles = Object.freeze([
  '.planning/phases/FLOWPDF-05-semantic-fillable-forms/05-VALIDATION.md',
  '.planning/phases/FLOWPDF-05-semantic-fillable-forms/05-CLOSURE.md',
  'scripts/check-phase5.mjs',
  'crates/flow-core/src/forms/mod.rs',
  'crates/flow-core/tests/forms.rs',
  'crates/flow-core/tests/field_authoring.rs',
  'crates/flow-core/tests/field_accessibility.rs',
  'crates/flow-core/tests/pdf_forms.rs',
  'crates/flow-core/tests/pdf_recovery.rs',
  'crates/flow-wasm/tests/form_projections.rs',
  'crates/flow-wasm/tests/form_sessions.rs',
  'web/tests/form-controls.browser.test.ts',
  'web/tests/form-projection.browser.test.ts',
  'web/tests/form-projection.test.ts',
  'web/tests/form-session.test.ts',
  'web/tests/pdf-request.browser.test.ts',
  'web/tests/pdf-request.test.ts',
  'web/tests/pdf-preview.browser.test.ts',
  'web/tests/pdf-worker.test.ts',
  'web/tests/production-runtime.browser.test.ts',
  'web/src/main.tsx',
  'web/src/layout/layout-worker.ts',
  'web/src/layout/layout-runtime.ts',
  'web/src/layout/layout-worker-entry.ts',
  'web/src/pdf/pdf-worker.ts',
  'web/src/pdf/pdf-worker-entry.ts',
  'web/src/runtime/font-catalog.ts',
])

function nodeStep(args) {
  return { command: process.execPath, args, env: process.env }
}

function npmStep(args) {
  return { command: npmBinary, args, env: process.env }
}

function npxStep(args) {
  return { command: npxBinary, args, env: browserEnvironment }
}

function cargoStep(args) {
  const withLock = args.includes('--locked') ? args : [args[0], '--locked', ...args.slice(1)]
  return { command: cargoBinary, args: withLock, env: rustEnvironment }
}

function gitStep(args) {
  return { command: 'git', args, env: process.env }
}

function task(id, commandText, steps, mode = 'run') {
  return Object.freeze({ id, commandText, mode, steps: Object.freeze(steps) })
}

export const phaseFiveValidationTasks = Object.freeze([
  task(
    '05-01-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test forms --test field_authoring --test field_accessibility -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'forms', '--test', 'field_authoring', '--test', 'field_accessibility', '--', '--nocapture'])],
  ),
  task(
    '05-02-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_forms --test pdf_recovery -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'pdf_forms', '--test', 'pdf_recovery', '--', '--nocapture'])],
  ),
  task(
    '05-03-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --test form_projections --test form_sessions -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-wasm', '--test', 'form_projections', '--test', 'form_sessions', '--', '--nocapture'])],
  ),
  task(
    '05-04-01',
    'npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/form-projection.test.ts web/tests/form-session.test.ts',
    [
      npmStep(['run', 'typecheck']),
      npxStep(['--no-install', 'vitest', 'run', '--project', 'unit', 'web/tests/form-projection.test.ts', 'web/tests/form-session.test.ts']),
    ],
  ),
  task(
    '05-05-01',
    'PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/form-controls.browser.test.ts web/tests/form-projection.browser.test.ts',
    [npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/form-controls.browser.test.ts', 'web/tests/form-projection.browser.test.ts'])],
  ),
  task(
    '05-06-01',
    'PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/pdf-request.test.ts web/tests/pdf-worker.test.ts && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/pdf-request.browser.test.ts web/tests/pdf-preview.browser.test.ts',
    [
      npxStep(['--no-install', 'vitest', 'run', '--project', 'unit', 'web/tests/pdf-request.test.ts', 'web/tests/pdf-worker.test.ts']),
      npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/pdf-request.browser.test.ts', 'web/tests/pdf-preview.browser.test.ts']),
    ],
  ),
  task(
    '05-07-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --quiet && RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --quiet',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--quiet']), cargoStep(['test', '--locked', '-p', 'flow-wasm', '--quiet'])],
  ),
  task('05-08-01', 'npm run test:unit', [npmStep(['run', 'test:unit'])]),
  task(
    '05-09-01',
    'node --test tests/contracts/phase1-boundary.test.mjs tests/contracts/phase2-boundary.test.mjs && git diff --check',
    [nodeStep(['--test', 'tests/contracts/phase1-boundary.test.mjs', 'tests/contracts/phase2-boundary.test.mjs']), gitStep(['diff', '--check'])],
  ),
  task('05-10-01', 'npm run build:web', [npmStep(['run', 'build:web'])]),
  task(
    '05-11-01',
    'PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/production-runtime.browser.test.ts',
    [npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/production-runtime.browser.test.ts'])],
  ),
  task('05-12-01', 'npm run check:phase5:smoke && npm run check:phase5 && npm run check:phase5', [], 'terminal'),
])

export function parseValidationCommands(markdown = readFileSync(phaseValidationPath, 'utf8')) {
  return [...markdown.matchAll(/^\| (05-[0-9]{2}-[0-9]{2}) \|[^\n]*?\| `([^`]+)` \|$/gm)].map(
    ([, id, commandText]) => ({ id, commandText }),
  )
}

export function assertValidationManifest(tasks = phaseFiveValidationTasks) {
  const planned = parseValidationCommands()
  const actual = tasks.map(({ id, commandText }) => ({ id, commandText }))
  if (JSON.stringify(actual) !== JSON.stringify(planned)) {
    throw new Error('Phase 5 validation task IDs/commands drifted from 05-VALIDATION.md')
  }
  if (new Set(tasks.map(({ id }) => id)).size !== tasks.length) {
    throw new Error('Phase 5 validation task IDs are not unique')
  }
  if (tasks.at(-1)?.mode !== 'terminal') {
    throw new Error('Phase 5 terminal task must remain an orchestration-only command')
  }
  return true
}

export function assertSafeDiagnostic(diagnostic) {
  const allowedKeys = new Set(['id', 'status', 'exitCode', 'elapsedMilliseconds'])
  for (const key of Object.keys(diagnostic)) {
    if (!allowedKeys.has(key)) throw new Error(`unsafe gate diagnostic key: ${key}`)
  }
  if (typeof diagnostic.id !== 'string' || !/^05-[0-9]{2}-[0-9]{2}$/.test(diagnostic.id)) {
    throw new Error('unsafe gate diagnostic task id')
  }
  if (!['pass', 'fail', 'skipped'].includes(diagnostic.status)) {
    throw new Error('unsafe gate diagnostic status')
  }
  if (!Number.isInteger(diagnostic.exitCode) || !Number.isFinite(diagnostic.elapsedMilliseconds)) {
    throw new Error('unsafe gate diagnostic numeric value')
  }
  return true
}

function phaseFivePlanIds() {
  return readdirSync(phaseDirectory)
    .map((name) => /^05-([0-9]{2})-PLAN\.md$/.exec(name))
    .filter(Boolean)
    .map(([, number]) => Number(number))
    .sort((left, right) => left - right)
}

export function getPhaseFivePlanStatus() {
  const planIds = phaseFivePlanIds()
  const maxPlanId = planIds.at(-1) ?? 0
  const missingPlanIds = Array.from({ length: maxPlanId }, (_, index) => index + 1).filter(
    (number) => !planIds.includes(number),
  )
  const openPlanIds = planIds.filter((number) => {
    const summaryPath = resolve(phaseDirectory, `05-${String(number).padStart(2, '0')}-SUMMARY.md`)
    const summary = readFileIfPresent(summaryPath)
    if (!summary) return true
    return !/^(?:\*\*Status:\*\*|status:)\s+complete\b/im.test(summary)
  })
  return { planIds, openPlanIds, missingPlanIds }
}

function readFileIfPresent(path) {
  try {
    return readFileSync(path, 'utf8')
  } catch {
    return ''
  }
}

export function assertPhaseFiveClosed() {
  const { planIds, openPlanIds, missingPlanIds } = getPhaseFivePlanStatus()
  if (planIds.length === 0) throw new Error('Phase 5 has no executable plan inventory')
  if (missingPlanIds.length > 0) {
    throw new Error(`Phase 5 plan inventory has gaps: ${missingPlanIds.map((id) => `05-${String(id).padStart(2, '0')}`).join(', ')}`)
  }
  if (openPlanIds.length > 0) {
    throw new Error(`Phase 5 has open plans: ${openPlanIds.map((id) => `05-${String(id).padStart(2, '0')}`).join(', ')}`)
  }
  const closure = readFileIfPresent(phaseClosurePath)
  if (!/^# Phase 5 Closure Record/m.test(closure) || !/^## Local closure/m.test(closure)) {
    throw new Error('Phase 5 closure record is missing or incomplete')
  }
  return true
}

function digestFile(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

export function buildGateFingerprint() {
  assertValidationManifest()
  const { planIds, openPlanIds, missingPlanIds } = getPhaseFivePlanStatus()
  return {
    validationTaskIds: phaseFiveValidationTasks.map(({ id }) => id),
    coverage: phaseFiveCoverageSummary,
    planIds,
    openPlanIds,
    missingPlanIds,
    artifactHashes: Object.fromEntries(fingerprintFiles.map((relativePath) => [relativePath, digestFile(resolve(projectRoot, relativePath))])),
  }
}

export function runPhaseFiveGate({ output = process.stdout, requireClosure = true } = {}) {
  assertValidationManifest()
  if (requireClosure) assertPhaseFiveClosed()
  const before = buildGateFingerprint()
  output.write(`Phase 5 preflight local=${phaseFiveCoverageSummary.local}\n`)
  for (const planTask of phaseFiveValidationTasks) {
    if (planTask.mode === 'terminal') continue
    output.write(`[${planTask.id}] start\n`)
    for (const stepDefinition of planTask.steps) {
      const { diagnostic, details } = runBoundedStep({
        id: planTask.id,
        command: stepDefinition.command,
        args: stepDefinition.args,
        cwd: projectRoot,
        env: stepDefinition.env ?? process.env,
      })
      assertSafeDiagnostic(diagnostic)
      if (diagnostic.status === 'fail') {
        writeFailureDetails(output, planTask.id, details)
        output.write(`[${planTask.id}] fail exit=${diagnostic.exitCode}\n`)
        return diagnostic.exitCode
      }
    }
    output.write(`[${planTask.id}] pass\n`)
  }
  const after = buildGateFingerprint()
  if (JSON.stringify(before) !== JSON.stringify(after)) {
    throw new Error('Phase 5 validation/fingerprint changed during one gate run')
  }
  output.write(`Phase 5 gate passed localTasks=${phaseFiveValidationTasks.length - 1}\n`)
  return 0
}

export function runPhaseFivePreflight({ output = process.stdout } = {}) {
  assertValidationManifest()
  assertPhaseFiveClosed()
  output.write('Phase 5 closure preflight passed\n')
  return 0
}

const isMain = process.argv[1] !== undefined && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))

if (isMain) {
  try {
    process.exitCode = process.argv.includes('--preflight')
      ? runPhaseFivePreflight()
      : runPhaseFiveGate({ requireClosure: !process.argv.includes('--allow-open-plan') })
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
    process.exitCode = 1
  }
}
