#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { delimiter, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { performance } from 'node:perf_hooks'

import { assertSupportedNodeVersion } from './node-version.mjs'
import {
  buildPhaseTwoUiContract,
  validatePhaseTwoUiContract,
} from './build-phase2-ui-contract.mjs'
import { runPhaseTwoScaleSmoke } from './verify-phase2-scale.mjs'

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
  '.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-VALIDATION.md',
)

function loadPhaseTwoAtEvidence(root = projectRoot) {
  const readJsonBlock = (relativePath) => {
    const markdown = readFileSync(resolve(root, relativePath), 'utf8')
    const match = markdown.match(/```json\n([\s\S]*?)\n```/)
    if (match === null) throw new Error(`missing AT evidence JSON block: ${relativePath}`)
    return JSON.parse(match[1])
  }
  return {
    local: readJsonBlock('tests/manual/phase2-local-voiceover.md'),
    external: readJsonBlock('tests/manual/phase2-edge-windows-screen-reader.md'),
  }
}

function validatePhaseTwoAtEvidence(evidence) {
  if (evidence?.local?.phase !== 'FLOWPDF-02-accessible-rich-text-editing') {
    throw new Error('local AT evidence has the wrong phase')
  }
  if (!['observed', 'not-run', 'unavailable'].includes(evidence.local.status)) {
    throw new Error('local AT evidence has an invalid status')
  }
  if (evidence.local.status !== 'observed' && evidence.local.observed !== false) {
    throw new Error('local non-observed AT evidence cannot claim observed=true')
  }
  if (
    evidence.local.status === 'observed' &&
    (evidence.local.observed !== true ||
      !/VoiceOver/i.test(String(evidence.local.target)) ||
      !/VoiceOver/i.test(String(evidence.local.environment?.browser)))
  ) {
    throw new Error('observed local AT evidence must name an exercised VoiceOver target')
  }
  if (evidence.external?.target !== 'Microsoft Edge on Windows with a Windows screen reader') {
    throw new Error('external AT evidence target is not the required checkpoint')
  }
  if (evidence.external.status !== 'unavailable' || evidence.external.closure !== 'outstanding') {
    throw new Error('external Edge/Windows AT evidence must remain unavailable/outstanding')
  }
  if (evidence.external.observed !== false || evidence.external.substitutionForbidden?.length !== 2) {
    throw new Error('external AT evidence is incomplete')
  }
  if (/\b(pass|passed|complete|closed)\b/i.test(JSON.stringify(evidence.external))) {
    throw new Error('external AT evidence contains a fabricated pass/completion claim')
  }
  return true
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

function task(id, commandText, steps, mode = 'run') {
  return Object.freeze({
    id,
    commandText,
    mode,
    steps: Object.freeze(steps),
  })
}

// These commandText values are copied character-for-character from the owning
// task rows in 02-VALIDATION.md. The executable steps use pinned binaries and
// shell=false; the text remains the auditable planning contract.
export const phaseTwoValidationTasks = Object.freeze([
  task('02-01-01', 'node --test scripts/verify-phase2-dependencies.test.mjs && node scripts/verify-phase2-dependencies.mjs', [
    nodeStep(['--test', 'scripts/verify-phase2-dependencies.test.mjs']),
    nodeStep(['scripts/verify-phase2-dependencies.mjs']),
  ]),
  task('02-01-02', 'npm ci --ignore-scripts && node scripts/verify-dependency-locks.mjs && npm ls --all && RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo tree --locked -e features', [
    npmStep(['ci', '--ignore-scripts']),
    nodeStep(['scripts/verify-dependency-locks.mjs']),
    npmStep(['ls', '--all']),
    cargoStep(['tree', '--locked', '-e', 'features']),
  ]),
  task('02-02-01', 'node --test scripts/verify-unicode-corpus.test.mjs && node scripts/verify-unicode-corpus.mjs', [
    nodeStep(['--test', 'scripts/verify-unicode-corpus.test.mjs']),
    nodeStep(['scripts/verify-unicode-corpus.mjs']),
  ]),
  task('02-02-02', 'node scripts/verify-wasm-size.mjs && node --test tests/contracts/phase1-boundary.test.mjs && npm run check', [
    nodeStep(['scripts/verify-wasm-size.mjs']),
    nodeStep(['--test', 'tests/contracts/phase1-boundary.test.mjs']),
    npmStep(['run', 'check']),
  ]),
  task('02-03-01', 'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test schema_v2_migration -- legacy_freeze_gate --nocapture', [
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'schema_v2_migration', '--', 'legacy_freeze_gate', '--nocapture']),
  ]),
  task('02-03-02', 'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test schema_v2_migration -- migration_routes --nocapture && RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test schema --test persistence_round_trip', [
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'schema_v2_migration', '--', 'migration_routes', '--nocapture']),
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'schema', '--test', 'persistence_round_trip']),
  ]),
  task('02-04-01', 'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test grapheme_conformance -- --nocapture', [
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'grapheme_conformance', '--', '--nocapture']),
  ]),
  task('02-04-02', 'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test editor_session -- --nocapture && npm run build:wasm && npm run typecheck', [
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'editor_session', '--', '--nocapture']),
    npmStep(['run', 'build:wasm']),
    npmStep(['run', 'typecheck']),
  ]),
  task('02-05-01', 'cargo test -p flow-core --test rich_text_transactions -- paragraph_tracer --nocapture && npm run build:wasm', [
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'rich_text_transactions', '--', 'paragraph_tracer', '--nocapture']),
    npmStep(['run', 'build:wasm']),
  ]),
  task('02-05-02', 'npm run build:web && npx --no-install vitest run --project browser web/tests/walking-skeleton.browser.test.ts --testNamePattern "Phase 2 paragraph tracer"', [
    npmStep(['run', 'build:web']),
    npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/walking-skeleton.browser.test.ts', '--testNamePattern', 'Phase 2 paragraph tracer']),
  ]),
  task('02-06-01', 'npm run typecheck && npx --no-install vitest run web/tests/editor-controller.test.ts', [
    npmStep(['run', 'typecheck']),
    npxStep(['--no-install', 'vitest', 'run', 'web/tests/editor-controller.test.ts']),
  ]),
  task('02-06-02', 'npx --no-install vitest run --project browser web/tests/editor-input.browser.test.ts && node scripts/verify-ime-chromium.mjs', [
    npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/editor-input.browser.test.ts']),
    nodeStep(['scripts/verify-ime-chromium.mjs']),
  ]),
  task('02-07-01', 'cargo test -p flow-core --test rich_text_transactions --test rich_text_properties -- structural_algebra --nocapture', [
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'rich_text_transactions', '--test', 'rich_text_properties', '--', 'structural_algebra', '--nocapture']),
  ]),
  task('02-07-02', 'cargo test -p flow-core --test rich_text_properties --test recovery -- deletion_preimage --nocapture', [
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'rich_text_properties', '--test', 'recovery', '--', 'deletion_preimage', '--nocapture']),
  ]),
  task('02-08-01', 'npx --no-install vitest run --project browser web/tests/rich-text-structure.browser.test.ts --testNamePattern "structural keys and controls"', [
    npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/rich-text-structure.browser.test.ts', '--testNamePattern', 'structural keys and controls']),
  ]),
  task('02-08-02', 'npx --no-install vitest run --project browser web/tests/rich-text-structure.browser.test.ts --testNamePattern "durable structural parity"', [
    npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/rich-text-structure.browser.test.ts', '--testNamePattern', 'durable structural parity']),
  ]),
  task('02-09-01', 'cargo test -p flow-core --test rich_text_formatting --test rich_text_properties -- formatting --nocapture', [
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'rich_text_formatting', '--test', 'rich_text_properties', '--', 'formatting', '--nocapture']),
  ]),
  task('02-09-02', 'cargo test -p flow-core --test rich_text_formatting --test rich_text_properties -- list --nocapture', [
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'rich_text_formatting', '--test', 'rich_text_properties', '--', 'list', '--nocapture']),
  ]),
  task('02-10-01', 'npx --no-install vitest run --project browser web/tests/formatting-toolbar.browser.test.ts', [
    npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/formatting-toolbar.browser.test.ts']),
  ]),
  task('02-10-02', 'npx --no-install vitest run --project browser web/tests/rich-text-formatting.browser.test.ts', [
    npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/rich-text-formatting.browser.test.ts']),
  ]),
  task('02-11-01', 'cargo test -p flow-core --test structural_blocks --test rich_text_properties -- page_break --nocapture', [
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'structural_blocks', '--test', 'rich_text_properties', '--', 'page_break', '--nocapture']),
  ]),
  task('02-11-02', 'cargo test -p flow-core --test structural_blocks --test rich_text_properties -- table --nocapture', [
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'structural_blocks', '--test', 'rich_text_properties', '--', 'table', '--nocapture']),
  ]),
  task('02-12-01', 'npx --no-install vitest run --project browser web/tests/table-accessibility.browser.test.ts', [
    npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/table-accessibility.browser.test.ts']),
  ]),
  task('02-12-02', 'npx --no-install vitest run --project browser web/tests/structural-blocks.browser.test.ts', [
    npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/structural-blocks.browser.test.ts']),
  ]),
  task('02-13-01', 'cargo test -p flow-core --test asset_staging --test recovery -- stage_insert --nocapture', [
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'asset_staging', '--test', 'recovery', '--', 'stage_insert', '--nocapture']),
  ]),
  task('02-13-02', 'cargo test -p flow-core --test asset_staging --test recovery -- image_lifecycle --nocapture', [
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'asset_staging', '--test', 'recovery', '--', 'image_lifecycle', '--nocapture']),
  ]),
  task('02-14-01', 'npx --no-install vitest run web/tests/indexeddb-store.test.ts && cargo test -p flow-core --test recovery -- image_asset --nocapture', [
    npxStep(['--no-install', 'vitest', 'run', 'web/tests/indexeddb-store.test.ts']),
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'recovery', '--', 'image_asset', '--nocapture']),
  ]),
  task('02-14-02', 'npx --no-install vitest run --project browser web/tests/image-lifecycle.browser.test.ts', [
    npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/image-lifecycle.browser.test.ts']),
  ]),
  task('02-15-01', 'cargo test -p flow-core --test command_parity -- --nocapture', [
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'command_parity', '--', '--nocapture']),
  ]),
  task('02-15-02', 'node --test tests/contracts/phase2-boundary.test.mjs tests/contracts/phase1-boundary.test.mjs', [
    nodeStep(['--test', 'tests/contracts/phase2-boundary.test.mjs', 'tests/contracts/phase1-boundary.test.mjs']),
  ]),
  task('02-16-01', 'cargo test -p flow-core --test field_accessibility -- --nocapture', [
    cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'field_accessibility', '--', '--nocapture']),
  ]),
  task('02-16-02', 'npx --no-install vitest run --project browser web/tests/editor-accessibility.browser.test.ts', [
    npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/editor-accessibility.browser.test.ts']),
  ]),
  task('02-17-01', 'node --test tests/contracts/phase2-ui-contract.test.mjs && node scripts/build-phase2-ui-contract.mjs --check', [
    nodeStep(['--test', 'tests/contracts/phase2-ui-contract.test.mjs']),
    nodeStep(['scripts/build-phase2-ui-contract.mjs', '--check']),
  ]),
  task('02-17-02', 'npx --no-install vitest run --project browser web/tests/editor-responsive.browser.test.ts web/tests/editor-ui-states.browser.test.ts && node --test tests/contracts/phase2-at-evidence.test.mjs', [
    npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/editor-responsive.browser.test.ts', 'web/tests/editor-ui-states.browser.test.ts']),
    nodeStep(['--test', 'tests/contracts/phase2-at-evidence.test.mjs']),
  ]),
  task('02-18-01', 'node --test tests/contracts/phase2-scale.test.mjs && node scripts/verify-phase2-scale.mjs --smoke', [
    nodeStep(['--test', 'tests/contracts/phase2-scale.test.mjs']),
    nodeStep(['scripts/verify-phase2-scale.mjs', '--smoke']),
  ]),
  task('02-18-02', 'npm run check:phase2:smoke && npm run check:phase2 && npm run check:phase2', [], 'terminal'),
])

export const phaseTwoCoverageSummary = Object.freeze({
  requirements: '7/7',
  edges: '35/35',
  ui: '108/108/0/0/0',
  prohibitions: '3/3 flagged',
  externalAt: 'unavailable/outstanding',
})

export function parseValidationCommands(markdown = readFileSync(phaseValidationPath, 'utf8')) {
  return [...markdown.matchAll(/^\| (02-[0-9]{2}-[0-9]{2}) \|[^\n]*?\| `([^`]+)` \|[^\n]*$/gm)].map(
    ([, id, commandText]) => ({ id, commandText }),
  )
}

export function assertValidationManifest(tasks = phaseTwoValidationTasks) {
  const planned = parseValidationCommands()
  const actual = tasks.map(({ id, commandText }) => ({ id, commandText }))
  if (JSON.stringify(actual) !== JSON.stringify(planned)) {
    throw new Error('Phase 2 validation task IDs/commands drifted from 02-VALIDATION.md')
  }
  if (new Set(tasks.map(({ id }) => id)).size !== tasks.length) {
    throw new Error('Phase 2 validation task IDs are not unique')
  }
  if (tasks.at(-1)?.mode !== 'terminal') {
    throw new Error('Phase 2 terminal task must remain an orchestration-only command')
  }
  return true
}

export function buildGateFingerprint(root = projectRoot) {
  const uiContract = buildPhaseTwoUiContract({ root })
  validatePhaseTwoUiContract(uiContract, { root })
  const scale = runPhaseTwoScaleSmoke(root)
  const evidence = loadPhaseTwoAtEvidence(root)
  validatePhaseTwoAtEvidence(evidence)
  return {
    validationTaskIds: phaseTwoValidationTasks.map(({ id }) => id),
    coverage: phaseTwoCoverageSummary,
    uiSourceDigest: uiContract.source.digest,
    uiEntryCount: uiContract.entries.length,
    scaleHashes: scale.semanticPayloadHashes,
    scaleRecoveryHashes: scale.recoveryHashes,
    externalAt: evidence.external.status + '/' + evidence.external.closure,
  }
}

export function assertSafeDiagnostic(diagnostic) {
  const allowedKeys = new Set(['id', 'status', 'exitCode', 'elapsedMilliseconds'])
  for (const key of Object.keys(diagnostic)) {
    if (!allowedKeys.has(key)) throw new Error(`unsafe gate diagnostic key: ${key}`)
  }
  if (typeof diagnostic.id !== 'string' || !/^02-[0-9]{2}-[0-9]{2}$/.test(diagnostic.id)) {
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

export function runPhaseTwoGate({ output = process.stdout } = {}) {
  assertValidationManifest()
  const before = buildGateFingerprint()
  output.write(
    `Phase 2 preflight requirements=${phaseTwoCoverageSummary.requirements} ` +
      `edges=${phaseTwoCoverageSummary.edges} ui=${phaseTwoCoverageSummary.ui} ` +
      `prohibitions=${phaseTwoCoverageSummary.prohibitions} externalAT=${phaseTwoCoverageSummary.externalAt}\n`,
  )
  for (const planTask of phaseTwoValidationTasks) {
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
    throw new Error('Phase 2 generated contract/hash fingerprint changed during one gate run')
  }
  output.write(
    `Phase 2 gate passed tasks=${phaseTwoValidationTasks.length - 1} ` +
      `ui=${after.uiEntryCount} externalAT=${after.externalAt}\n`,
  )
  return 0
}

const isMain =
  process.argv[1] !== undefined &&
  resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))

if (isMain) {
  try {
    process.exitCode = runPhaseTwoGate()
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
    process.exitCode = 1
  }
}
