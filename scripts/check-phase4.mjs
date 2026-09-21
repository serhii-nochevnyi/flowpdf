#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { delimiter, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { performance } from 'node:perf_hooks'

import { assertSupportedNodeVersion } from './node-version.mjs'

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
const phaseValidationPath = resolve(
  projectRoot,
  '.planning/phases/FLOWPDF-04-owned-pdf-preview-and-export/04-VALIDATION.md',
)
const phaseWorkerPath = resolve(projectRoot, 'web/src/pdf/pdf-worker.ts')

export const phaseFourCoverageSummary = Object.freeze({
  requirements: '9/9 mapped',
  local: 'COS/resources/provenance/WASM/worker/preview',
  regressions: 'Phase 1/2/3 local gates retained',
  reference: 'pass/fail/unavailable; no tool or fixture is not a pass',
  phase2ExternalAt: 'unavailable/outstanding',
})

const fingerprintFiles = Object.freeze([
  '.planning/phases/FLOWPDF-04-owned-pdf-preview-and-export/04-VALIDATION.md',
  'scripts/check-phase4.mjs',
  'tests/contracts/phase4-gate.test.mjs',
  'tests/contracts/phase1-boundary.test.mjs',
  'tests/contracts/phase2-boundary.test.mjs',
  'crates/flow-core/src/pdf/mod.rs',
  'crates/flow-core/src/pdf/display_list.rs',
  'crates/flow-core/src/pdf/font.rs',
  'crates/flow-core/src/pdf/assets.rs',
  'crates/flow-core/src/pdf/metadata.rs',
  'crates/flow-core/src/pdf/provenance.rs',
  'crates/flow-core/src/pdf/recovery.rs',
  'crates/flow-core/tests/pdf_cos.rs',
  'crates/flow-core/tests/pdf_text.rs',
  'crates/flow-core/tests/pdf_resources.rs',
  'crates/flow-core/tests/pdf_recovery.rs',
  'crates/flow-wasm/src/lib.rs',
  'crates/flow-wasm/tests/pdf_exports.rs',
  'web/src/pdf/pdf-protocol.ts',
  'web/src/pdf/pdf-worker.ts',
  'web/src/pdf/pdf-preview.tsx',
  'web/src/pdf/pdf-preview.css',
  'web/src/editor/editor-controller.ts',
  'web/src/editor/editor-store.ts',
  'web/src/editor/editor-app.tsx',
  'web/tests/pdf-worker.test.ts',
  'web/tests/pdf-preview.browser.test.ts',
])

function loadPhase2ExternalEvidence(root = projectRoot) {
  const markdown = readFileSync(
    resolve(root, 'tests/manual/phase2-edge-windows-screen-reader.md'),
    'utf8',
  )
  const match = markdown.match(/```json\n([\s\S]*?)\n```/)
  if (match === null) throw new Error('missing Phase 2 external AT evidence JSON')
  const evidence = JSON.parse(match[1])
  if (
    evidence?.target !== 'Microsoft Edge on Windows with a Windows screen reader' ||
    evidence.status !== 'unavailable' ||
    evidence.observed !== false ||
    evidence.closure !== 'outstanding' ||
    evidence.substitutionForbidden?.length !== 2
  ) {
    throw new Error('Phase 2 external AT evidence is not honestly unavailable/outstanding')
  }
  if (/\b(pass|passed|complete|closed)\b/i.test(JSON.stringify(evidence))) {
    throw new Error('Phase 2 external AT evidence contains a fabricated completion claim')
  }
  return evidence
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

export const phaseFourValidationTasks = Object.freeze([
  task(
    '04-01-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_cos -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'pdf_cos', '--', '--nocapture'])],
  ),
  task(
    '04-02-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_text -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'pdf_text', '--', '--nocapture'])],
  ),
  task(
    '04-03-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_resources -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'pdf_resources', '--', '--nocapture'])],
  ),
  task(
    '04-04-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test pdf_recovery -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'pdf_recovery', '--', '--nocapture'])],
  ),
  task(
    '04-05-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --test pdf_exports -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-wasm', '--test', 'pdf_exports', '--', '--nocapture'])],
  ),
  task(
    '04-05-02',
    'npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/pdf-worker.test.ts',
    [
      npmStep(['run', 'typecheck']),
      npxStep(['--no-install', 'vitest', 'run', '--project', 'unit', 'web/tests/pdf-worker.test.ts']),
    ],
  ),
  task(
    '04-05-03',
    'npm run build:web && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/pdf-preview.browser.test.ts web/tests/layout-viewport.browser.test.ts web/tests/editor-accessibility.browser.test.ts',
    [
      npmStep(['run', 'build:web']),
      npxStep([
        '--no-install',
        'vitest',
        'run',
        '--project',
        'browser',
        'web/tests/pdf-preview.browser.test.ts',
        'web/tests/layout-viewport.browser.test.ts',
        'web/tests/editor-accessibility.browser.test.ts',
      ]),
    ],
  ),
  task(
    '04-06-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --quiet && RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --quiet',
    [
      cargoStep(['test', '--locked', '-p', 'flow-core', '--quiet']),
      cargoStep(['test', '--locked', '-p', 'flow-wasm', '--quiet']),
    ],
  ),
  task('04-06-02', 'npm run test:unit', [npmStep(['run', 'test:unit'])]),
  task('04-06-03', 'npm run check:phase3', [npmStep(['run', 'check:phase3'])]),
  task('04-06-04', 'git diff --check', [gitStep(['diff', '--check'])]),
  task(
    '04-06-05',
    'npm run check:phase4:smoke && npm run check:phase4 && npm run check:phase4',
    [],
    'terminal',
  ),
])

export const phaseFourReferenceTasks = Object.freeze([
  Object.freeze({
    id: '04-06-R1',
    lane: 'Structural PDF/reference',
    tool: 'qpdf',
    artifact: 'artifacts/phase4/flowpdf.pdf',
    commandText: 'qpdf --check artifacts/phase4/flowpdf.pdf',
    args: ['--check', 'artifacts/phase4/flowpdf.pdf'],
  }),
  Object.freeze({
    id: '04-06-R2',
    lane: 'Text extraction/reference',
    tool: 'pdftotext',
    artifact: 'artifacts/phase4/flowpdf.pdf',
    commandText: 'pdftotext artifacts/phase4/flowpdf.pdf artifacts/phase4/flowpdf.txt',
    args: ['artifacts/phase4/flowpdf.pdf', 'artifacts/phase4/flowpdf.txt'],
  }),
  Object.freeze({
    id: '04-06-R3',
    lane: 'Visual raster/reference',
    tool: 'pdftoppm',
    artifact: 'artifacts/phase4/flowpdf.pdf',
    commandText: 'pdftoppm -png artifacts/phase4/flowpdf.pdf artifacts/phase4/render',
    args: ['-png', 'artifacts/phase4/flowpdf.pdf', 'artifacts/phase4/render'],
  }),
  Object.freeze({
    id: '04-06-R4',
    lane: 'Target viewer visual/reference',
    tool: 'Microsoft Edge on Windows',
    artifact: 'artifacts/phase4/flowpdf.pdf',
    commandText: 'target-viewer visual-diff evidence for artifacts/phase4/flowpdf.pdf',
    args: [],
  }),
])

export function parseValidationCommands(markdown = readFileSync(phaseValidationPath, 'utf8')) {
  return [...markdown.matchAll(/^\| (04-[0-9]{2}-[0-9]{2}) \|[^\n]*?\| `([^`]+)` \|$/gm)].map(
    ([, id, commandText]) => ({ id, commandText }),
  )
}

export function parseReferenceCommands(markdown = readFileSync(phaseValidationPath, 'utf8')) {
  return [
    ...markdown.matchAll(
      /^\| (04-06-R[0-9]+) \|[^\n]*?\| `([^`]+)` \| `([^`]+)` \| `([^`]+)` \|$/gm,
    ),
  ].map(([, id, tool, artifact, commandText]) => ({ id, tool, artifact, commandText }))
}

export function assertNoFutureScope(markdown = readFileSync(phaseValidationPath, 'utf8')) {
  const contract = markdown.split('## Evidence boundary')[0] ?? markdown
  if (/\b(?:Phase [5-9]|AcroForm|OCR|voice|PDF\/A|PDF\/UA|external PDF import)\b/i.test(contract)) {
    throw new Error('Phase 4 validation manifest leaks future or deferred scope')
  }
  return true
}

export function assertValidationManifest(
  tasks = phaseFourValidationTasks,
  references = phaseFourReferenceTasks,
) {
  const planned = parseValidationCommands()
  const actual = tasks.map(({ id, commandText }) => ({ id, commandText }))
  if (JSON.stringify(actual) !== JSON.stringify(planned)) {
    throw new Error('Phase 4 validation task IDs/commands drifted from 04-VALIDATION.md')
  }
  if (new Set(tasks.map(({ id }) => id)).size !== tasks.length) {
    throw new Error('Phase 4 validation task IDs are not unique')
  }
  if (tasks.at(-1)?.mode !== 'terminal') {
    throw new Error('Phase 4 terminal task must remain an orchestration-only command')
  }

  const plannedReferences = parseReferenceCommands()
  const actualReferences = references.map(({ id, tool, artifact, commandText }) => ({
    id,
    tool,
    artifact,
    commandText,
  }))
  if (JSON.stringify(actualReferences) !== JSON.stringify(plannedReferences)) {
    throw new Error('Phase 4 reference rows drifted from 04-VALIDATION.md')
  }
  if (new Set(references.map(({ id }) => id)).size !== references.length) {
    throw new Error('Phase 4 reference task IDs are not unique')
  }
  assertNoFutureScope()
  return true
}

export function assertSafeDiagnostic(diagnostic) {
  const allowedKeys = new Set(['id', 'status', 'exitCode', 'elapsedMilliseconds'])
  for (const key of Object.keys(diagnostic)) {
    if (!allowedKeys.has(key)) throw new Error(`unsafe gate diagnostic key: ${key}`)
  }
  if (typeof diagnostic.id !== 'string' || !/^04-(?:[0-9]{2}-[0-9]{2}|06-R[0-9]+)$/.test(diagnostic.id)) {
    throw new Error('unsafe gate diagnostic task id')
  }
  if (!['pass', 'fail', 'unavailable', 'skipped'].includes(diagnostic.status)) {
    throw new Error('unsafe gate diagnostic status')
  }
  if (!Number.isInteger(diagnostic.exitCode) || !Number.isFinite(diagnostic.elapsedMilliseconds)) {
    throw new Error('unsafe gate diagnostic numeric value')
  }
  return true
}

export function assertReferenceDiagnostic(
  diagnostic,
  { toolAvailable = false, artifactAvailable = false } = {},
) {
  assertSafeDiagnostic(diagnostic)
  if (diagnostic.status === 'pass' && (!toolAvailable || !artifactAvailable)) {
    throw new Error('unavailable reference evidence cannot be recorded as pass')
  }
  return true
}

export function assertRevisionSafeWorkerContract(source = readFileSync(phaseWorkerPath, 'utf8')) {
  const required = [
    'validatePdfExportResult(request, result)',
    'controller.signal.aborted',
    'this.isCurrent(generation, request.requestId)',
    'this.acceptedValue = accepted',
  ]
  for (const marker of required) {
    if (!source.includes(marker)) {
      throw new Error(`PDF worker stale-result guard missing: ${marker}`)
    }
  }
  return true
}

export function buildGateFingerprint(root = projectRoot) {
  const external = loadPhase2ExternalEvidence(root)
  assertValidationManifest()
  assertRevisionSafeWorkerContract(readFileSync(resolve(root, 'web/src/pdf/pdf-worker.ts'), 'utf8'))
  const artifactHashes = Object.fromEntries(
    fingerprintFiles.map((relativePath) => [relativePath, digestFile(resolve(root, relativePath))]),
  )
  return {
    validationTaskIds: phaseFourValidationTasks.map(({ id }) => id),
    referenceTaskIds: phaseFourReferenceTasks.map(({ id }) => id),
    coverage: phaseFourCoverageSummary,
    artifactHashes,
    phase2ExternalAt: `${external.status}/${external.closure}`,
  }
}

function digestFile(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

function runStep(taskId, stepDefinition) {
  const startedAt = performance.now()
  const result = spawnSync(stepDefinition.command, stepDefinition.args, {
    cwd: projectRoot,
    env: stepDefinition.env ?? process.env,
    shell: false,
    stdio: ['ignore', 'ignore', 'ignore'],
  })
  const elapsedMilliseconds = Math.round(performance.now() - startedAt)
  const exitCode = Number.isInteger(result.status) ? result.status : 1
  const diagnostic = {
    id: taskId,
    status: exitCode === 0 ? 'pass' : 'fail',
    exitCode,
    elapsedMilliseconds,
  }
  assertSafeDiagnostic(diagnostic)
  return diagnostic
}

function toolAvailable(tool) {
  if (tool === 'Microsoft Edge on Windows') return false
  const result = spawnSync('which', [tool], { cwd: projectRoot, stdio: 'ignore', shell: false })
  return result.status === 0
}

export function runReferenceTask(referenceTask) {
  const startedAt = performance.now()
  const availableTool = toolAvailable(referenceTask.tool)
  const availableArtifact = existsSync(resolve(projectRoot, referenceTask.artifact))
  if (!availableTool || !availableArtifact) {
    const diagnostic = {
      id: referenceTask.id,
      status: 'unavailable',
      exitCode: 127,
      elapsedMilliseconds: Math.round(performance.now() - startedAt),
    }
    assertReferenceDiagnostic(diagnostic, {
      toolAvailable: availableTool,
      artifactAvailable: availableArtifact,
    })
    return diagnostic
  }
  const result = spawnSync(referenceTask.tool, referenceTask.args, {
    cwd: projectRoot,
    env: process.env,
    shell: false,
    stdio: ['ignore', 'ignore', 'ignore'],
  })
  const diagnostic = {
    id: referenceTask.id,
    status: result.status === 0 ? 'pass' : 'fail',
    exitCode: Number.isInteger(result.status) ? result.status : 1,
    elapsedMilliseconds: Math.round(performance.now() - startedAt),
  }
  assertReferenceDiagnostic(diagnostic, {
    toolAvailable: availableTool,
    artifactAvailable: availableArtifact,
  })
  return diagnostic
}

export function runPhaseFourGate({ output = process.stdout } = {}) {
  assertValidationManifest()
  const before = buildGateFingerprint()
  output.write(
    `Phase 4 preflight requirements=${phaseFourCoverageSummary.requirements} ` +
      `local=${phaseFourCoverageSummary.local} regressions=${phaseFourCoverageSummary.regressions} ` +
      `phase2ExternalAT=${before.phase2ExternalAt}\n`,
  )
  for (const planTask of phaseFourValidationTasks) {
    if (planTask.mode === 'terminal') continue
    output.write(`[${planTask.id}] start\n`)
    for (const stepDefinition of planTask.steps) {
      const diagnostic = runStep(planTask.id, stepDefinition)
      if (diagnostic.status === 'fail') {
        output.write(`[${planTask.id}] fail exit=${diagnostic.exitCode}\n`)
        return diagnostic.exitCode
      }
    }
    output.write(`[${planTask.id}] pass\n`)
  }

  let unavailableReferences = 0
  for (const referenceTask of phaseFourReferenceTasks) {
    const diagnostic = runReferenceTask(referenceTask)
    if (diagnostic.status === 'unavailable') unavailableReferences += 1
    output.write(`[${referenceTask.id}] ${diagnostic.status}\n`)
    if (diagnostic.status === 'fail') return diagnostic.exitCode
  }

  const after = buildGateFingerprint()
  if (JSON.stringify(before) !== JSON.stringify(after)) {
    throw new Error('Phase 4 artifact/coverage fingerprint changed during one gate run')
  }
  output.write(
    `Phase 4 gate passed localTasks=${phaseFourValidationTasks.length - 1} ` +
      `referenceUnavailable=${unavailableReferences}/${phaseFourReferenceTasks.length} ` +
      `requirements=${phaseFourCoverageSummary.requirements} ` +
      `phase2ExternalAT=${after.phase2ExternalAt}\n`,
  )
  return 0
}

const isMain =
  process.argv[1] !== undefined &&
  resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))

if (isMain) {
  try {
    process.exitCode = runPhaseFourGate()
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
    process.exitCode = 1
  }
}
