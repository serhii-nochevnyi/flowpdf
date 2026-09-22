#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { readFileSync, readdirSync, statSync } from 'node:fs'
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
const phaseDirectory = resolve(projectRoot, '.planning/phases/FLOWPDF-06-voice-dictation-and-commands')
const phaseValidationPath = resolve(phaseDirectory, '06-VALIDATION.md')

export const phaseSixCoverageSummary = Object.freeze({
  requirements: 'VOIC-01..06 locally covered by typed Rust/WASM, controller, recognition, and UI tests',
  local: 'Rust/WASM/unit/browser/build/boundary/privacy',
  external: 'speech-service/permission and external AT unavailable; retained as release inputs',
})

const fingerprintFiles = Object.freeze([
  '.planning/phases/FLOWPDF-06-voice-dictation-and-commands/06-VALIDATION.md',
  'scripts/check-phase6.mjs',
  'tests/contracts/phase1-boundary.test.mjs',
  'tests/contracts/phase6-gate.test.mjs',
  'crates/flow-core/src/voice/mod.rs',
  'crates/flow-core/tests/voice.rs',
  'crates/flow-wasm/tests/voice_exports.rs',
  'web/src/voice/voice-command.ts',
  'web/src/voice/voice-protocol.ts',
  'web/src/voice/voice-recognition.ts',
  'web/src/editor/editor-controller.ts',
  'web/src/editor/editor-app.tsx',
  'web/src/editor/editor-messages.ts',
  'web/src/editor/editor.css',
  'web/src/editor/voice-controls.tsx',
  'web/tests/voice-command.test.ts',
  'web/tests/voice-recognition.test.ts',
  'web/tests/voice-recognition.browser.test.ts',
  'web/tests/voice-controls.browser.test.ts',
  'web/tests/editor-ui-states.browser.test.ts',
  'package.json',
  'package-lock.json',
])

const privacySourcePattern = /^(?:web\/src\/voice\/[A-Za-z0-9._/-]+\.ts|web\/src\/editor\/voice-controls\.tsx)$/
const forbiddenPrivacyPatterns = Object.freeze([
  ['browser storage', /\b(?:localStorage|sessionStorage|indexedDB|IDBDatabase)\b/i],
  ['network/telemetry', /\b(?:fetch|XMLHttpRequest|sendBeacon|WebSocket|EventSource|analytics|telemetry|gtag|plausible)\b/i],
  ['independent audio capture', /\b(?:getUserMedia|mediaDevices|MediaRecorder|AudioContext)\b/i],
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

export const phaseSixValidationTasks = Object.freeze([
  task(
    '06-01-01',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --test voice -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--test', 'voice', '--', '--nocapture'])],
  ),
  task(
    '06-01-02',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --test voice_exports -- --nocapture',
    [cargoStep(['test', '--locked', '-p', 'flow-wasm', '--test', 'voice_exports', '--', '--nocapture'])],
  ),
  task(
    '06-02-01',
    'npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/voice-command.test.ts web/tests/editor-controller.test.ts',
    [
      npmStep(['run', 'typecheck']),
      npxStep(['--no-install', 'vitest', 'run', '--project', 'unit', 'web/tests/voice-command.test.ts', 'web/tests/editor-controller.test.ts']),
    ],
  ),
  task(
    '06-03-01',
    'PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/voice-recognition.test.ts',
    [npxStep(['--no-install', 'vitest', 'run', '--project', 'unit', 'web/tests/voice-recognition.test.ts'])],
  ),
  task(
    '06-03-02',
    'PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/voice-recognition.browser.test.ts',
    [npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/voice-recognition.browser.test.ts'])],
  ),
  task(
    '06-04-01',
    'PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/form-session.test.ts web/tests/voice-command.test.ts web/tests/editor-controller.test.ts',
    [npxStep(['--no-install', 'vitest', 'run', '--project', 'unit', 'web/tests/form-session.test.ts', 'web/tests/voice-command.test.ts', 'web/tests/editor-controller.test.ts'])],
  ),
  task(
    '06-05-01',
    'npm run typecheck && PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project unit web/tests/voice-command.test.ts',
    [
      npmStep(['run', 'typecheck']),
      npxStep(['--no-install', 'vitest', 'run', '--project', 'unit', 'web/tests/voice-command.test.ts']),
    ],
  ),
  task(
    '06-05-02',
    'PLAYWRIGHT_BROWSERS_PATH=./work/playwright npx --no-install vitest run --project browser web/tests/voice-controls.browser.test.ts web/tests/editor-ui-states.browser.test.ts',
    [npxStep(['--no-install', 'vitest', 'run', '--project', 'browser', 'web/tests/voice-controls.browser.test.ts', 'web/tests/editor-ui-states.browser.test.ts'])],
  ),
  task(
    '06-06-01',
    'node --test tests/contracts/phase6-gate.test.mjs tests/contracts/phase1-boundary.test.mjs && git diff --check',
    [
      nodeStep(['--test', 'tests/contracts/phase6-gate.test.mjs', 'tests/contracts/phase1-boundary.test.mjs']),
      gitStep(['diff', '--check']),
    ],
  ),
  task('06-06-02', 'npm run test:unit', [npmStep(['run', 'test:unit'])]),
  task(
    '06-06-03',
    'RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-core --quiet && RUSTUP_HOME=./work/toolchains/rustup CARGO_HOME=./work/toolchains/cargo PATH=./work/toolchains/cargo/bin:$PATH cargo test --locked -p flow-wasm --quiet',
    [cargoStep(['test', '--locked', '-p', 'flow-core', '--quiet']), cargoStep(['test', '--locked', '-p', 'flow-wasm', '--quiet'])],
  ),
  task('06-06-04', 'npm run build:web', [npmStep(['run', 'build:web'])]),
  task('06-06-05', 'npm run test:browser', [npmStep(['run', 'test:browser'])]),
  task('06-06-06', 'npm run check', [npmStep(['run', 'check'])]),
  task('06-06-07', 'npm run check:phase6 && npm run check:phase6 && npm run check:planning', [], 'terminal'),
])

export function parseValidationCommands(markdown = readFileSync(phaseValidationPath, 'utf8')) {
  return [...markdown.matchAll(/^\| (06-[0-9]{2}-[0-9]{2}) \|[^\n]*?\| `([^`]+)` \|$/gm)].map(
    ([, id, commandText]) => ({ id, commandText }),
  )
}

export function assertValidationManifest(tasks = phaseSixValidationTasks) {
  const planned = parseValidationCommands()
  const actual = tasks.map(({ id, commandText }) => ({ id, commandText }))
  if (JSON.stringify(actual) !== JSON.stringify(planned)) {
    throw new Error('Phase 6 validation task IDs/commands drifted from 06-VALIDATION.md')
  }
  if (new Set(tasks.map(({ id }) => id)).size !== tasks.length) {
    throw new Error('Phase 6 validation task IDs are not unique')
  }
  if (tasks.at(-1)?.mode !== 'terminal') {
    throw new Error('Phase 6 terminal task must remain an orchestration-only command')
  }
  return true
}

export function assertSafeDiagnostic(diagnostic) {
  const allowedKeys = new Set(['id', 'status', 'exitCode', 'elapsedMilliseconds'])
  for (const key of Object.keys(diagnostic)) {
    if (!allowedKeys.has(key)) throw new Error(`unsafe gate diagnostic key: ${key}`)
  }
  if (typeof diagnostic.id !== 'string' || !/^06-[0-9]{2}-[0-9]{2}$/.test(diagnostic.id)) {
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

export function getPhaseSixPlanStatus() {
  const planIds = readdirSync(phaseDirectory)
    .map((name) => /^06-([0-9]{2})-PLAN\.md$/.exec(name))
    .filter(Boolean)
    .map(([, number]) => Number(number))
    .sort((left, right) => left - right)
  const maxPlanId = planIds.at(-1) ?? 0
  const missingPlanIds = Array.from({ length: maxPlanId }, (_, index) => index + 1).filter(
    (number) => !planIds.includes(number),
  )
  const openPlanIds = planIds.filter((number) => {
    const summaryPath = resolve(phaseDirectory, `06-${String(number).padStart(2, '0')}-SUMMARY.md`)
    const summary = readFileIfPresent(summaryPath)
    return !/^(?:\*\*Status:\*\*|status:)\s+complete\b/im.test(summary)
  })
  return { planIds, openPlanIds, missingPlanIds }
}

export function assertPhaseSixClosed() {
  const { planIds, openPlanIds, missingPlanIds } = getPhaseSixPlanStatus()
  if (planIds.length === 0) throw new Error('Phase 6 has no executable plan inventory')
  if (missingPlanIds.length > 0) {
    throw new Error(`Phase 6 plan inventory has gaps: ${missingPlanIds.map((id) => `06-${String(id).padStart(2, '0')}`).join(', ')}`)
  }
  if (openPlanIds.length > 0) {
    throw new Error(`Phase 6 has open plans: ${openPlanIds.map((id) => `06-${String(id).padStart(2, '0')}`).join(', ')}`)
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

function productionFiles(directory) {
  const files = []
  for (const entry of readdirSync(directory)) {
    const path = resolve(directory, entry)
    const stat = statSync(path)
    if (stat.isDirectory()) {
      files.push(...productionFiles(path))
    } else if (/\.(?:ts|tsx)$/.test(entry)) {
      files.push(path)
    }
  }
  return files
}

function loadVoiceSources(root = projectRoot) {
  const sourceFiles = [
    ...productionFiles(resolve(root, 'web/src/voice')),
    resolve(root, 'web/src/editor/voice-controls.tsx'),
  ]
  return new Map(
    sourceFiles.map((path) => [relative(root, path), readFileSync(path, 'utf8')]),
  )
}

export function voicePrivacyDiagnostics(snapshot = { typescript: loadVoiceSources() }) {
  const diagnostics = []
  for (const [path, source] of snapshot.typescript) {
    if (!privacySourcePattern.test(path)) continue
    for (const [label, pattern] of forbiddenPrivacyPatterns) {
      if (pattern.test(source)) diagnostics.push(`${path}: forbidden ${label} surface`)
    }
  }
  return diagnostics.sort()
}

function digestFile(path) {
  return createHash('sha256').update(readFileSync(path)).digest('hex')
}

export function buildGateFingerprint() {
  assertValidationManifest()
  const { planIds, openPlanIds, missingPlanIds } = getPhaseSixPlanStatus()
  const privacyDiagnostics = voicePrivacyDiagnostics()
  if (privacyDiagnostics.length > 0) {
    throw new Error(`Phase 6 privacy boundary failed:\n${privacyDiagnostics.join('\n')}`)
  }
  return {
    validationTaskIds: phaseSixValidationTasks.map(({ id }) => id),
    coverage: phaseSixCoverageSummary,
    planIds,
    openPlanIds,
    missingPlanIds,
    privacy: 'clean',
    artifactHashes: Object.fromEntries(
      fingerprintFiles.map((relativePath) => [relativePath, digestFile(resolve(projectRoot, relativePath))]),
    ),
  }
}

export function runPhaseSixGate({ output = process.stdout, requireClosure = true } = {}) {
  assertValidationManifest()
  if (requireClosure) assertPhaseSixClosed()
  const before = buildGateFingerprint()
  output.write(`Phase 6 preflight local=${phaseSixCoverageSummary.local}\n`)
  for (const planTask of phaseSixValidationTasks) {
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
    throw new Error('Phase 6 validation/fingerprint changed during one gate run')
  }
  output.write(`Phase 6 gate passed localTasks=${phaseSixValidationTasks.length - 1}\n`)
  return 0
}

export function runPhaseSixPreflight({ output = process.stdout } = {}) {
  assertValidationManifest()
  assertPhaseSixClosed()
  output.write('Phase 6 closure preflight passed\n')
  return 0
}

const isMain = process.argv[1] !== undefined && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))

if (isMain) {
  try {
    process.exitCode = process.argv.includes('--preflight')
      ? runPhaseSixPreflight()
      : runPhaseSixGate({ requireClosure: !process.argv.includes('--allow-open-plan') })
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
    process.exitCode = 1
  }
}
