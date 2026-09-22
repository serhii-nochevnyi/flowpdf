#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { readFileSync, readdirSync } from 'node:fs'
import { delimiter, dirname, relative, resolve } from 'node:path'
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
const phaseDirectory = resolve(projectRoot, '.planning/phases/FLOWPDF-07-secure-pdf-reader-and-scene')
const phaseValidationPath = resolve(phaseDirectory, '07-VALIDATION.md')
const phaseClosurePath = resolve(phaseDirectory, '07-CLOSURE.md')

export const phaseSevenCoverageSummary = Object.freeze({
  requirements: 'PDFI-01..03, PDFI-05, QUAL-05, QUAL-06 locally covered for the controlled reader subset',
  local: 'bounded Rust reader/scene, WASM, worker, report-first browser panel, regression, and planning',
  external: 'qpdf/Poppler/target-viewer and external AT remain unavailable release inputs unless observed separately',
})

const fingerprintFiles = Object.freeze([
  '.planning/phases/FLOWPDF-07-secure-pdf-reader-and-scene/07-VALIDATION.md',
  '.planning/phases/FLOWPDF-07-secure-pdf-reader-and-scene/07-CLOSURE.md',
  'scripts/check-phase7.mjs',
  'tests/contracts/phase7-gate.test.mjs',
  'tests/contracts/phase1-boundary.test.mjs',
  'tests/contracts/phase2-boundary.test.mjs',
  'config/dependency-provenance.json',
  'artifacts/provenance/phase1-dependencies.json',
  'artifacts/benchmarks/phase1-recovery.json',
  'crates/flow-core/src/pdf/reader.rs',
  'crates/flow-core/src/pdf/scene.rs',
  'crates/flow-core/tests/pdf_reader.rs',
  'crates/flow-core/tests/pdf_scene.rs',
  'crates/flow-wasm/src/lib.rs',
  'crates/flow-wasm/tests/pdf_reader.rs',
  'web/src/pdf/pdf-protocol.ts',
  'web/src/pdf/pdf-reader-worker.ts',
  'web/src/pdf/pdf-reader-worker-entry.ts',
  'web/src/pdf/pdf-reader-panel.tsx',
  'web/src/layout/layout-runtime.ts',
  'web/src/editor/editor-app.tsx',
  'web/tests/pdf-reader.test.ts',
  'web/tests/pdf-reader.browser.test.ts',
  'package.json',
  'package-lock.json',
])

const readerSourceFiles = Object.freeze([
  'crates/flow-core/src/pdf/reader.rs',
  'crates/flow-core/src/pdf/scene.rs',
  'crates/flow-wasm/src/lib.rs',
  'web/src/pdf/pdf-protocol.ts',
  'web/src/pdf/pdf-reader-worker.ts',
  'web/src/pdf/pdf-reader-worker-entry.ts',
  'web/src/pdf/pdf-reader-panel.tsx',
])

const forbiddenReaderPatterns = Object.freeze([
  ['network access', /\b(?:fetch|XMLHttpRequest|WebSocket|EventSource|sendBeacon)\s*\(/i],
  ['external navigation', /\b(?:window|globalThis|document)\.(?:open|location)\b|location\.(?:assign|replace)\s*\(/i],
  ['script execution', /\b(?:eval|Function)\s*\(/i],
  ['process/executable launch', /(?:std::process::Command|child_process|execFile|spawn)\b/i],
  ['action callback', /(?:javascript:|mailto:|onload\s*=|onclick\s*=)/],
  ['mutable reader handle', /(?:readerHandle|MutablePdfReader|new\s+PdfReader\s*\()/i],
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
  const withLock = args.includes('--locked') || args[0] === 'fmt'
    ? args
    : [args[0], '--locked', ...args.slice(1)]
  return { command: cargoBinary, args: withLock, env: rustEnvironment }
}

function gitStep(args) {
  return { command: 'git', args, env: process.env }
}

function task(id, commandText, steps, mode = 'run') {
  return Object.freeze({ id, commandText, mode, steps: Object.freeze(steps) })
}

export const phaseSevenValidationTasks = Object.freeze([
  task(
    '07-01-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_reader -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'pdf_reader', '--', '--nocapture'])],
  ),
  task(
    '07-02-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_scene -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'pdf_scene', '--', '--nocapture'])],
  ),
  task(
    '07-03-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --test pdf_reader -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-wasm', '--test', 'pdf_reader', '--', '--nocapture'])],
  ),
  task(
    '07-03-02',
    'npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/pdf-reader.test.ts',
    [
      npmStep(['run', 'typecheck']),
      npxStep(['--no-install', 'vitest', 'run', '--project', 'unit', 'web/tests/pdf-reader.test.ts']),
    ],
  ),
  task(
    '07-03-03',
    'npm run build:web && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/pdf-reader.browser.test.ts',
    [
      npmStep(['run', 'build:web']),
      npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/pdf-reader.browser.test.ts']),
    ],
  ),
  task(
    '07-04-01',
    'node --test tests/contracts/phase7-gate.test.mjs && git diff --check',
    [
      nodeStep(['--test', 'tests/contracts/phase7-gate.test.mjs']),
      gitStep(['diff', '--check']),
    ],
  ),
  task(
    '07-04-02',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --quiet && RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --quiet',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--quiet']), cargoStep(['test', '--locked', '-p', 'flow-wasm', '--quiet'])],
  ),
  task(
    '07-04-03',
    'npm run check:phase6 && npm run check:phase5 && npm run check:phase4',
    [npmStep(['run', 'check:phase6']), npmStep(['run', 'check:phase5']), npmStep(['run', 'check:phase4'])],
  ),
  task('07-04-04', 'npm run check:phase7 && npm run check:phase7 && npm run check:planning', [], 'terminal'),
  task(
    '07-04-05',
    'git diff --check && cargo fmt --all -- --check',
    [gitStep(['diff', '--check']), cargoStep(['fmt', '--all', '--', '--check'])],
  ),
])

export function parseValidationCommands(markdown = readFileSync(phaseValidationPath, 'utf8')) {
  return [...markdown.matchAll(/^\| (07-[0-9]{2}-[0-9]{2}) \|[^\n]*?\| `([^`]+)` \|$/gm)].map(
    ([, id, commandText]) => ({ id, commandText }),
  )
}

export function assertValidationManifest(tasks = phaseSevenValidationTasks) {
  const planned = parseValidationCommands()
  const actual = tasks.map(({ id, commandText }) => ({ id, commandText }))
  if (JSON.stringify(actual) !== JSON.stringify(planned)) {
    throw new Error('Phase 7 validation task IDs/commands drifted from 07-VALIDATION.md')
  }
  if (new Set(tasks.map(({ id }) => id)).size !== tasks.length) {
    throw new Error('Phase 7 validation task IDs are not unique')
  }
  if (tasks.filter(({ mode }) => mode === 'terminal').length !== 1) {
    throw new Error('Phase 7 must have exactly one terminal orchestration task')
  }
  return true
}

export function assertSafeDiagnostic(diagnostic) {
  const allowedKeys = new Set(['id', 'status', 'exitCode', 'elapsedMilliseconds'])
  for (const key of Object.keys(diagnostic)) {
    if (!allowedKeys.has(key)) throw new Error(`unsafe gate diagnostic key: ${key}`)
  }
  if (typeof diagnostic.id !== 'string' || !/^07-[0-9]{2}-[0-9]{2}$/.test(diagnostic.id)) {
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

function readFileIfPresent(path) {
  try {
    return readFileSync(path, 'utf8')
  } catch {
    return ''
  }
}

export function getPhaseSevenPlanStatus() {
  const planIds = readdirSync(phaseDirectory)
    .map((name) => /^07-([0-9]{2})-PLAN\.md$/.exec(name))
    .filter(Boolean)
    .map(([, number]) => Number(number))
    .sort((left, right) => left - right)
  const maxPlanId = planIds.at(-1) ?? 0
  const missingPlanIds = Array.from({ length: maxPlanId }, (_, index) => index + 1).filter(
    (number) => !planIds.includes(number),
  )
  const openPlanIds = planIds.filter((number) => {
    const summaryPath = resolve(phaseDirectory, `07-${String(number).padStart(2, '0')}-SUMMARY.md`)
    const summary = readFileIfPresent(summaryPath)
    return !/^(?:\*\*Status:\*\*|status:)\s+complete\b/im.test(summary)
  })
  return { planIds, openPlanIds, missingPlanIds }
}

export function readerBoundaryDiagnostics(snapshot = loadReaderSources()) {
  const diagnostics = []
  for (const [path, source] of snapshot) {
    for (const [label, pattern] of forbiddenReaderPatterns) {
      if (pattern.test(source)) diagnostics.push(`${path}: forbidden ${label} surface`)
    }
  }
  const diagnosticStruct = snapshot.get('crates/flow-core/src/pdf/reader.rs') ?? ''
  const diagnosticMatch = /pub struct PdfReadDiagnostic \{([\s\S]*?)\n\}/m.exec(diagnosticStruct)?.[1] ?? ''
  if (/\b(?:bytes|data|payload|canonical_json)\b/i.test(diagnosticMatch)) {
    diagnostics.push('PdfReadDiagnostic exposes raw input/payload fields')
  }
  const wasmSource = snapshot.get('crates/flow-wasm/src/lib.rs') ?? ''
  if (/PdfReader\s*(?:\{|\)|,)[\s\S]{0,160}wasm_bindgen/i.test(wasmSource)) {
    diagnostics.push('WASM boundary exposes a mutable PdfReader handle')
  }
  return diagnostics.sort()
}

function loadReaderSources(root = projectRoot) {
  return new Map(
    readerSourceFiles.map((relativePath) => [relativePath, readFileSync(resolve(root, relativePath), 'utf8')]),
  )
}

export function assertPhaseSevenClosed() {
  const { planIds, openPlanIds, missingPlanIds } = getPhaseSevenPlanStatus()
  if (JSON.stringify(planIds) !== JSON.stringify([1, 2, 3, 4])) {
    throw new Error(`Phase 7 plan inventory is not [1,2,3,4]: ${planIds.join(',')}`)
  }
  if (missingPlanIds.length > 0) {
    throw new Error(`Phase 7 plan inventory has gaps: ${missingPlanIds.join(',')}`)
  }
  if (openPlanIds.length > 0) {
    throw new Error(`Phase 7 has open plans: ${openPlanIds.join(',')}`)
  }
  const closure = readFileIfPresent(phaseClosurePath)
  if (!/^# Phase 7 Closure Record/m.test(closure) || !/^## Local closure/m.test(closure)) {
    throw new Error('Phase 7 closure record is missing or incomplete')
  }
  return true
}

function digestFile(path) {
  return createHash('sha256').update(readFileIfPresent(path)).digest('hex')
}

export function buildGateFingerprint() {
  assertValidationManifest()
  const planStatus = getPhaseSevenPlanStatus()
  const privacyDiagnostics = readerBoundaryDiagnostics()
  if (privacyDiagnostics.length > 0) {
    throw new Error(`Phase 7 reader boundary failed:\n${privacyDiagnostics.join('\n')}`)
  }
  return {
    validationTaskIds: phaseSevenValidationTasks.map(({ id }) => id),
    coverage: phaseSevenCoverageSummary,
    ...planStatus,
    boundary: 'clean',
    artifactHashes: Object.fromEntries(
      fingerprintFiles.map((relativePath) => [relativePath, digestFile(resolve(projectRoot, relativePath))]),
    ),
  }
}

export function runPhaseSevenGate({ output = process.stdout, requireClosure = true } = {}) {
  assertValidationManifest()
  if (requireClosure) assertPhaseSevenClosed()
  const before = buildGateFingerprint()
  output.write(`Phase 7 preflight local=${phaseSevenCoverageSummary.local}\n`)
  for (const planTask of phaseSevenValidationTasks) {
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
    throw new Error('Phase 7 validation/fingerprint changed during one gate run')
  }
  output.write(`Phase 7 gate passed localTasks=${phaseSevenValidationTasks.length - 1}\n`)
  return 0
}

export function runPhaseSevenPreflight({ output = process.stdout } = {}) {
  assertValidationManifest()
  assertPhaseSevenClosed()
  output.write('Phase 7 closure preflight passed\n')
  return 0
}

const isMain = process.argv[1] !== undefined && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))

if (isMain) {
  try {
    process.exitCode = process.argv.includes('--preflight')
      ? runPhaseSevenPreflight()
      : runPhaseSevenGate({ requireClosure: !process.argv.includes('--allow-open-plan') })
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
    process.exitCode = 1
  }
}
