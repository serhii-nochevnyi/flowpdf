#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
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
  '.planning/phases/FLOWPDF-03-deterministic-reflow-and-pagination/03-VALIDATION.md',
)

export const phaseThreeCoverageSummary = Object.freeze({
  requirements: '8/8 local',
  rust: 'deterministic/pinned',
  worker: 'revision-safe',
  browser: 'Chromium/pinned-cache',
  phase2ExternalAt: 'unavailable/outstanding',
})

const fingerprintFiles = Object.freeze([
  '.planning/phases/FLOWPDF-03-deterministic-reflow-and-pagination/03-VALIDATION.md',
  'crates/flow-core/src/layout/mod.rs',
  'crates/flow-core/src/invalidation/mod.rs',
  'crates/flow-core/tests/layout_pagination.rs',
  'crates/flow-core/tests/layout_constraints.rs',
  'crates/flow-core/tests/layout_equivalence.rs',
  'crates/flow-core/tests/layout_properties.rs',
  'crates/flow-wasm/src/lib.rs',
  'crates/flow-wasm/tests/layout_exports.rs',
  'web/src/layout/layout-protocol.ts',
  'web/src/layout/layout-worker.ts',
  'web/src/layout/page-viewport.tsx',
  'web/src/layout/page-viewport.css',
  'web/src/editor/editor-controller.ts',
  'web/src/editor/editor-store.ts',
  'web/src/editor/semantic-document.tsx',
  'fixtures/layout/phase3-pagination-corpus.json',
  'fixtures/layout/phase3-text-corpus.json',
  'fixtures/layout/font-provenance.json',
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

// These commandText values are copied character-for-character from the owning
// rows in 03-VALIDATION.md. The executable steps use shell=false and pinned
// environments; the text remains the auditable planning contract.
export const phaseThreeValidationTasks = Object.freeze([
  task(
    '03-01-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test layout_fixed_point --test layout_text -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'layout_fixed_point', '--test', 'layout_text', '--', '--nocapture'])],
  ),
  task(
    '03-01-02',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test grapheme_conformance --test layout_text -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'grapheme_conformance', '--test', 'layout_text', '--', '--nocapture'])],
  ),
  task(
    '03-02-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test schema_v2_migration --test schema_v3_layout -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'schema_v2_migration', '--test', 'schema_v3_layout', '--', '--nocapture'])],
  ),
  task(
    '03-03-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test layout_pagination --test layout_constraints -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'layout_pagination', '--test', 'layout_constraints', '--', '--nocapture'])],
  ),
  task(
    '03-04-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test layout_equivalence --test layout_properties -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'layout_equivalence', '--test', 'layout_properties', '--', '--nocapture'])],
  ),
  task(
    '03-05-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --test layout_exports -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-wasm', '--test', 'layout_exports', '--', '--nocapture'])],
  ),
  task('03-05-02', 'npm run build:wasm', [npmStep(['run', 'build:wasm'])]),
  task(
    '03-05-03',
    'npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run web/tests/layout-worker.test.ts',
    [
      npmStep(['run', 'typecheck']),
      npxStep(['--no-install', 'vitest', 'run', 'web/tests/layout-worker.test.ts']),
    ],
  ),
  task(
    '03-06-01',
    'npm run build:web && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/layout-viewport.browser.test.ts web/tests/editor-accessibility.browser.test.ts',
    [
      npmStep(['run', 'build:web']),
      npxStep([
        '--no-install',
        'vitest',
        'run',
        '--project',
        'browser',
        'web/tests/layout-viewport.browser.test.ts',
        'web/tests/editor-accessibility.browser.test.ts',
      ]),
    ],
  ),
  task(
    '03-06-02',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --quiet && RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --quiet',
    [
      cargoStep(['test', '--locked', '-p', 'flow-core', '--quiet']),
      cargoStep(['test', '--locked', '-p', 'flow-wasm', '--quiet']),
    ],
  ),
  task(
    '03-06-03',
    'npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit',
    [
      npmStep(['run', 'typecheck']),
      npxStep(['--no-install', 'vitest', 'run', '--project', 'unit']),
    ],
  ),
  task(
    '03-06-04',
    'node --test tests/contracts/phase1-boundary.test.mjs tests/contracts/phase2-boundary.test.mjs',
    [nodeStep(['--test', 'tests/contracts/phase1-boundary.test.mjs', 'tests/contracts/phase2-boundary.test.mjs'])],
  ),
  task(
    '03-06-05',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo clippy --locked --workspace --all-targets -- -D warnings',
    [cargoStep(['clippy', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings'])],
  ),
  task('03-06-06', 'npm run check', [npmStep(['run', 'check'])]),
  task('03-06-07', 'node scripts/verify-phase2-scale.mjs --smoke', [nodeStep(['scripts/verify-phase2-scale.mjs', '--smoke'])]),
  task('03-06-08', 'git diff --check', [gitStep(['diff', '--check'])]),
  task(
    '03-06-09',
    'npm run check:phase3:smoke && npm run check:phase3 && npm run check:phase3',
    [],
    'terminal',
  ),
])

export function parseValidationCommands(markdown = readFileSync(phaseValidationPath, 'utf8')) {
  return [...markdown.matchAll(/^\| (03-[A-Z0-9-]+) \|[^\n]*?\| `([^`]+)` \|[^\n]*$/gm)].map(
    ([, id, commandText]) => ({ id, commandText }),
  )
}

export function assertValidationManifest(tasks = phaseThreeValidationTasks) {
  const planned = parseValidationCommands()
  const actual = tasks.map(({ id, commandText }) => ({ id, commandText }))
  if (JSON.stringify(actual) !== JSON.stringify(planned)) {
    throw new Error('Phase 3 validation task IDs/commands drifted from 03-VALIDATION.md')
  }
  if (new Set(tasks.map(({ id }) => id)).size !== tasks.length) {
    throw new Error('Phase 3 validation task IDs are not unique')
  }
  if (tasks.at(-1)?.mode !== 'terminal') {
    throw new Error('Phase 3 terminal task must remain an orchestration-only command')
  }
  return true
}

export function assertSafeDiagnostic(diagnostic) {
  const allowedKeys = new Set(['id', 'status', 'exitCode', 'elapsedMilliseconds'])
  for (const key of Object.keys(diagnostic)) {
    if (!allowedKeys.has(key)) throw new Error(`unsafe gate diagnostic key: ${key}`)
  }
  if (typeof diagnostic.id !== 'string' || !/^03-[A-Z0-9-]+$/.test(diagnostic.id)) {
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

export function buildGateFingerprint(root = projectRoot) {
  const external = loadPhase2ExternalEvidence(root)
  assertValidationManifest()
  const artifactHashes = Object.fromEntries(
    fingerprintFiles.map((relativePath) => [relativePath, digestFile(resolve(root, relativePath))]),
  )
  return {
    validationTaskIds: phaseThreeValidationTasks.map(({ id }) => id),
    coverage: phaseThreeCoverageSummary,
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

export function runPhaseThreeGate({ output = process.stdout } = {}) {
  assertValidationManifest()
  const before = buildGateFingerprint()
  output.write(
    `Phase 3 preflight requirements=${phaseThreeCoverageSummary.requirements} ` +
      `rust=${phaseThreeCoverageSummary.rust} worker=${phaseThreeCoverageSummary.worker} ` +
      `browser=${phaseThreeCoverageSummary.browser} ` +
      `phase2ExternalAT=${before.phase2ExternalAt}\n`,
  )
  for (const planTask of phaseThreeValidationTasks) {
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
  const after = buildGateFingerprint()
  if (JSON.stringify(before) !== JSON.stringify(after)) {
    throw new Error('Phase 3 artifact/coverage fingerprint changed during one gate run')
  }
  output.write(
    `Phase 3 gate passed localTasks=${phaseThreeValidationTasks.length - 1} ` +
      `requirements=${phaseThreeCoverageSummary.requirements} ` +
      `phase2ExternalAT=${after.phase2ExternalAt}\n`,
  )
  return 0
}

const isMain =
  process.argv[1] !== undefined &&
  resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))

if (isMain) {
  try {
    process.exitCode = runPhaseThreeGate()
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
    process.exitCode = 1
  }
}
