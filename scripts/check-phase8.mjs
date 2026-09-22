#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { readFileSync, readdirSync } from 'node:fs'
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
const phaseDirectory = resolve(projectRoot, '.planning/phases/FLOWPDF-08-external-reconstruction-and-ocr')
const phaseValidationPath = resolve(phaseDirectory, '08-VALIDATION.md')
const phaseClosurePath = resolve(phaseDirectory, '08-CLOSURE.md')

export const phaseEightCoverageSummary = Object.freeze({
  requirements: 'PDFI-04, PDFI-06..08, QUAL-01, QUAL-02 locally covered for the bounded reconstruction/OCR-adapter subset',
  local: 'immutable source store, Rust candidate/provenance, verified WASM boundary, cancellable worker, review panel, browser evidence, regression, and planning',
  external: 'OCR provider/corpus, qpdf/Poppler/target-viewer, and external AT remain unavailable release inputs unless observed separately',
})

const fingerprintFiles = Object.freeze([
  '.planning/REQUIREMENTS.md',
  '.planning/ROADMAP.md',
  '.planning/STATE.md',
  '.planning/phases/FLOWPDF-08-external-reconstruction-and-ocr/08-VALIDATION.md',
  '.planning/phases/FLOWPDF-08-external-reconstruction-and-ocr/08-CLOSURE.md',
  '.planning/phases/FLOWPDF-08-external-reconstruction-and-ocr/08-01-SUMMARY.md',
  '.planning/phases/FLOWPDF-08-external-reconstruction-and-ocr/08-02-SUMMARY.md',
  '.planning/phases/FLOWPDF-08-external-reconstruction-and-ocr/08-03-SUMMARY.md',
  '.planning/phases/FLOWPDF-08-external-reconstruction-and-ocr/08-04-SUMMARY.md',
  'scripts/check-phase8.mjs',
  'tests/contracts/phase8-gate.test.mjs',
  'tests/contracts/phase1-boundary.test.mjs',
  'tests/contracts/phase2-boundary.test.mjs',
  'config/dependency-provenance.json',
  'artifacts/provenance/phase1-dependencies.json',
  'artifacts/benchmarks/phase1-recovery.json',
  'crates/flow-core/src/pdf/reconstruction.rs',
  'crates/flow-core/src/pdf/reader.rs',
  'crates/flow-core/src/pdf/scene.rs',
  'crates/flow-core/tests/pdf_reconstruction.rs',
  'crates/flow-core/tests/provenance.rs',
  'crates/flow-wasm/src/lib.rs',
  'crates/flow-wasm/tests/pdf_reconstruction.rs',
  'web/persistence/pdf-source-store.ts',
  'web/src/pdf/pdf-protocol.ts',
  'web/src/pdf/pdf-reader-worker.ts',
  'web/src/pdf/pdf-reader-panel.tsx',
  'web/src/pdf/pdf-reconstruction-protocol.ts',
  'web/src/pdf/pdf-reconstruction-worker.ts',
  'web/src/pdf/pdf-reconstruction-worker-entry.ts',
  'web/src/pdf/pdf-reconstruction-panel.tsx',
  'web/src/layout/layout-runtime.ts',
  'web/src/editor/editor-app.tsx',
  'web/tests/pdf-reconstruction.test.ts',
  'web/tests/pdf-reconstruction.browser.test.ts',
  'web/tests/pdf-source-store.test.ts',
  'package.json',
  'package-lock.json',
])

const reconstructionSourceFiles = Object.freeze([
  'crates/flow-core/src/pdf/reconstruction.rs',
  'crates/flow-core/src/pdf/reader.rs',
  'crates/flow-core/src/pdf/scene.rs',
  'crates/flow-wasm/src/lib.rs',
  'web/persistence/pdf-source-store.ts',
  'web/src/pdf/pdf-reconstruction-protocol.ts',
  'web/src/pdf/pdf-reconstruction-worker.ts',
  'web/src/pdf/pdf-reconstruction-worker-entry.ts',
  'web/src/pdf/pdf-reconstruction-panel.tsx',
])

const forbiddenReconstructionPatterns = Object.freeze([
  ['network access', /\b(?:fetch|XMLHttpRequest|WebSocket|EventSource|sendBeacon)\s*\(/i],
  ['external navigation', /\b(?:window|globalThis|document)\.(?:open|location)\b|location\.(?:assign|replace)\s*\(/i],
  ['script execution', /\b(?:eval|Function)\s*\(/i],
  ['process/executable launch', /(?:std::process::Command|child_process|execFile|spawn)\b/i],
  ['external action', /(?:javascript:|mailto:|\bonload\s*=|\bonclick\s*=)/],
  ['mutable reconstruction handle', /(?:reconstructionHandle|MutablePdfReconstruction|new\s+PdfReconstruction\s*\()/i],
  [
    'positive exact claim',
    /\b(?:exact|lossless|pixel[- ]perfect)\s+(?:reconstruction|conversion|parity)\s+(?:is\s+)?(?:guaranteed|supported|achieved|complete|available|confirmed)\b/i,
  ],
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

export const phaseEightValidationTasks = Object.freeze([
  task(
    '08-01-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_reconstruction -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'pdf_reconstruction', '--', '--nocapture'])],
  ),
  task(
    '08-01-02',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test provenance -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'provenance', '--', '--nocapture'])],
  ),
  task(
    '08-02-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --test pdf_reconstruction -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-wasm', '--test', 'pdf_reconstruction', '--', '--nocapture'])],
  ),
  task(
    '08-02-02',
    'npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/pdf-reconstruction.test.ts',
    [
      npmStep(['run', 'typecheck']),
      npxStep(['--no-install', 'vitest', 'run', '--project', 'unit', 'web/tests/pdf-reconstruction.test.ts']),
    ],
  ),
  task(
    '08-03-01',
    'npm run build:web && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/pdf-reconstruction.browser.test.ts',
    [
      npmStep(['run', 'build:web']),
      npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/pdf-reconstruction.browser.test.ts']),
    ],
  ),
  task(
    '08-03-02',
    'PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/pdf-source-store.test.ts',
    [npxStep(['--no-install', 'vitest', 'run', '--project', 'unit', 'web/tests/pdf-source-store.test.ts'])],
  ),
  task(
    '08-04-01',
    'node --test tests/contracts/phase8-gate.test.mjs && git diff --check',
    [nodeStep(['--test', 'tests/contracts/phase8-gate.test.mjs']), gitStep(['diff', '--check'])],
  ),
  task(
    '08-04-02',
    'npm run check:phase7 && npm run check:phase6 && npm run check:phase5',
    [npmStep(['run', 'check:phase7']), npmStep(['run', 'check:phase6']), npmStep(['run', 'check:phase5'])],
  ),
  task(
    '08-04-03',
    'git diff --check && cargo fmt --all -- --check && npm run check:planning',
    [gitStep(['diff', '--check']), cargoStep(['fmt', '--all', '--', '--check']), npmStep(['run', 'check:planning'])],
  ),
  task(
    '08-04-04',
    'npm run check:phase8 && npm run check:phase8 && npm run check:planning',
    [],
    'terminal',
  ),
])

export function parseValidationCommands(markdown = readFileSync(phaseValidationPath, 'utf8')) {
  return [...markdown.matchAll(/^\| (08-[0-9]{2}-[0-9]{2}) \|[^\n]*?\| `([^`]+)` \|$/gm)].map(
    ([, id, commandText]) => ({ id, commandText }),
  )
}

export function assertValidationManifest(tasks = phaseEightValidationTasks) {
  const planned = parseValidationCommands()
  const actual = tasks.map(({ id, commandText }) => ({ id, commandText }))
  if (JSON.stringify(actual) !== JSON.stringify(planned)) {
    throw new Error('Phase 8 validation task IDs/commands drifted from 08-VALIDATION.md')
  }
  if (new Set(tasks.map(({ id }) => id)).size !== tasks.length) {
    throw new Error('Phase 8 validation task IDs are not unique')
  }
  if (tasks.filter(({ mode }) => mode === 'terminal').length !== 1) {
    throw new Error('Phase 8 must have exactly one terminal orchestration task')
  }
  return true
}

export function assertSafeDiagnostic(diagnostic) {
  const allowedKeys = new Set(['id', 'status', 'exitCode', 'elapsedMilliseconds'])
  for (const key of Object.keys(diagnostic)) {
    if (!allowedKeys.has(key)) throw new Error(`unsafe gate diagnostic key: ${key}`)
  }
  if (typeof diagnostic.id !== 'string' || !/^08-[0-9]{2}-[0-9]{2}$/.test(diagnostic.id)) {
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

export function getPhaseEightPlanStatus() {
  const planIds = readdirSync(phaseDirectory)
    .map((name) => /^08-([0-9]{2})-PLAN\.md$/.exec(name))
    .filter(Boolean)
    .map(([, number]) => Number(number))
    .sort((left, right) => left - right)
  const maxPlanId = planIds.at(-1) ?? 0
  const missingPlanIds = Array.from({ length: maxPlanId }, (_, index) => index + 1).filter(
    (number) => !planIds.includes(number),
  )
  const openPlanIds = planIds.filter((number) => {
    const summaryPath = resolve(phaseDirectory, `08-${String(number).padStart(2, '0')}-SUMMARY.md`)
    const summary = readFileIfPresent(summaryPath)
    return !/^(?:\*\*Status:\*\*|status:)\s+complete\b/im.test(summary)
  })
  return { planIds, openPlanIds, missingPlanIds }
}

function extractInterface(source, name) {
  return new RegExp(`interface\\s+${name}\\s*\\{([\\s\\S]*?)\\}`, 'm').exec(source)?.[1] ?? ''
}

export function reconstructionBoundaryDiagnostics(snapshot = loadReconstructionSources()) {
  const diagnostics = []
  for (const [path, source] of snapshot) {
    for (const [label, pattern] of forbiddenReconstructionPatterns) {
      if (pattern.test(source)) diagnostics.push(`${path}: forbidden ${label} surface`)
    }
  }

  const reconstructionDiagnostics = extractInterface(
    snapshot.get('web/src/pdf/pdf-reconstruction-protocol.ts') ?? '',
    'PdfReconstructionDiagnosticDto',
  )
  if (/\b(?:bytes|bytesHex|imageDataHex|pdfBytes|payload|text)\b/i.test(reconstructionDiagnostics)) {
    diagnostics.push('PdfReconstructionDiagnosticDto exposes source/OCR payload fields')
  }

  const resultInterface = extractInterface(
    snapshot.get('web/src/pdf/pdf-reconstruction-protocol.ts') ?? '',
    'PdfReconstructionResultDto',
  )
  if (/\b(?:bytes|bytesHex|pdfBytes|sourceBytes|payload)\b/i.test(resultInterface)) {
    diagnostics.push('PdfReconstructionResultDto exposes source payload fields')
  }

  const rustDiagnostic = /pub struct PdfReconstructionDiagnostic\s*\{([\s\S]*?)\n\}/m.exec(
    snapshot.get('crates/flow-core/src/pdf/reconstruction.rs') ?? '',
  )?.[1] ?? ''
  if (/\b(?:bytes|data|payload|canonical_json|text)\b/i.test(rustDiagnostic)) {
    diagnostics.push('PdfReconstructionDiagnostic exposes raw input/OCR payload fields')
  }

  const readerDiagnostic = /pub struct PdfReadDiagnostic\s*\{([\s\S]*?)\n\}/m.exec(
    snapshot.get('crates/flow-core/src/pdf/reader.rs') ?? '',
  )?.[1] ?? ''
  if (/\b(?:bytes|data|payload|canonical_json|text)\b/i.test(readerDiagnostic)) {
    diagnostics.push('PdfReadDiagnostic exposes raw input/payload fields')
  }

  const sourceStore = snapshot.get('web/persistence/pdf-source-store.ts') ?? ''
  if (!/objectStore\(STORE_NAME\)\.add\(candidate\)/.test(sourceStore)) {
    diagnostics.push('immutable source store does not use put-if-absent add')
  }
  if (!/return cloneBytes\(record\.bytes\)/.test(sourceStore)) {
    diagnostics.push('immutable source store does not clone bytes on read')
  }
  if (/\.delete\s*\(|objectStore\([^)]*\)\.put\s*\(/.test(sourceStore)) {
    diagnostics.push('immutable source store exposes overwrite/delete behavior')
  }

  return diagnostics.sort()
}

function loadReconstructionSources(root = projectRoot) {
  return new Map(
    reconstructionSourceFiles.map((relativePath) => [relativePath, readFileSync(resolve(root, relativePath), 'utf8')]),
  )
}

export function assertPhaseEightClosed() {
  const { planIds, openPlanIds, missingPlanIds } = getPhaseEightPlanStatus()
  if (JSON.stringify(planIds) !== JSON.stringify([1, 2, 3, 4])) {
    throw new Error(`Phase 8 plan inventory is not [1,2,3,4]: ${planIds.join(',')}`)
  }
  if (missingPlanIds.length > 0) {
    throw new Error(`Phase 8 plan inventory has gaps: ${missingPlanIds.join(',')}`)
  }
  if (openPlanIds.length > 0) {
    throw new Error(`Phase 8 has open plans: ${openPlanIds.join(',')}`)
  }
  const closure = readFileIfPresent(phaseClosurePath)
  if (!/^# Phase 8 Closure Record/m.test(closure) || !/^## Local closure/m.test(closure)) {
    throw new Error('Phase 8 closure record is missing or incomplete')
  }
  return true
}

function digestFile(path) {
  return createHash('sha256').update(readFileIfPresent(path)).digest('hex')
}

export function buildGateFingerprint() {
  assertValidationManifest()
  const planStatus = getPhaseEightPlanStatus()
  const boundaryDiagnostics = reconstructionBoundaryDiagnostics()
  if (boundaryDiagnostics.length > 0) {
    throw new Error(`Phase 8 reconstruction boundary failed:\n${boundaryDiagnostics.join('\n')}`)
  }
  return {
    validationTaskIds: phaseEightValidationTasks.map(({ id }) => id),
    coverage: phaseEightCoverageSummary,
    ...planStatus,
    boundary: 'clean',
    artifactHashes: Object.fromEntries(
      fingerprintFiles.map((relativePath) => [relativePath, digestFile(resolve(projectRoot, relativePath))]),
    ),
  }
}

export function runPhaseEightGate({ output = process.stdout, requireClosure = true } = {}) {
  assertValidationManifest()
  if (requireClosure) assertPhaseEightClosed()
  const before = buildGateFingerprint()
  output.write(`Phase 8 preflight local=${phaseEightCoverageSummary.local}\n`)
  for (const planTask of phaseEightValidationTasks) {
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
    throw new Error('Phase 8 validation/fingerprint changed during one gate run')
  }
  output.write(`Phase 8 gate passed localTasks=${phaseEightValidationTasks.length - 1}\n`)
  return 0
}

export function runPhaseEightPreflight({ output = process.stdout } = {}) {
  assertValidationManifest()
  assertPhaseEightClosed()
  output.write('Phase 8 closure preflight passed\n')
  return 0
}

const isMain = process.argv[1] !== undefined && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))

if (isMain) {
  try {
    process.exitCode = process.argv.includes('--preflight')
      ? runPhaseEightPreflight()
      : runPhaseEightGate({ requireClosure: !process.argv.includes('--allow-open-plan') })
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
    process.exitCode = 1
  }
}
