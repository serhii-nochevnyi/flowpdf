#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import { existsSync, readFileSync } from 'node:fs'
import { delimiter, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { performance } from 'node:perf_hooks'

import { assertSupportedNodeVersion } from './node-version.mjs'

assertSupportedNodeVersion()
const projectRoot = resolve(import.meta.dirname, '..')
const cargoHome = resolve(projectRoot, 'work/toolchains/cargo')
const rustupHome = resolve(projectRoot, 'work/toolchains/rustup')
const cargoBinary = resolve(cargoHome, 'bin/cargo')
const npmBinary = process.platform === 'win32' ? 'npm.cmd' : 'npm'
const rustEnvironment = Object.freeze({
  ...process.env,
  CARGO_HOME: cargoHome,
  RUSTUP_HOME: rustupHome,
  PATH: `${dirname(cargoBinary)}${delimiter}${process.env.PATH ?? ''}`,
})

export const phaseOneSteps = Object.freeze([
  step(
    'dependency-provenance',
    'Dependency provenance verifier tests',
    process.execPath,
    ['--test', 'scripts/verify-dependency-provenance.mjs'],
  ),
  step('dependency-locks', 'Pinned dependency lock verification', process.execPath, [
    'scripts/verify-dependency-locks.mjs',
  ]),
  step('boundary-contract', 'Phase 1 ownership and deferred-scope contract', process.execPath, [
    '--test',
    'tests/contracts/phase1-boundary.test.mjs',
  ]),
  step('rust-format', 'Rust formatting', cargoBinary, ['fmt', '--all', '--', '--check'], {
    env: rustEnvironment,
  }),
  step(
    'rust-clippy',
    'Rust Clippy (all targets, warnings denied)',
    cargoBinary,
    ['clippy', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings'],
    { env: rustEnvironment },
  ),
  step(
    'rust-tests',
    'Full Rust golden, property, recovery, and redaction tests',
    cargoBinary,
    ['test', '--locked', '--workspace', '--all-targets'],
    { env: rustEnvironment },
  ),
  step('recovery-benchmark', 'Recovery benchmark terminal evidence', process.execPath, [
    'scripts/verify-recovery-benchmark.mjs',
    '--self-test',
  ], { env: rustEnvironment }),
  step(
    'wasm-target',
    'WASM target boundary check',
    cargoBinary,
    ['check', '--locked', '-p', 'flow-wasm', '--target', 'wasm32-unknown-unknown'],
    { env: rustEnvironment },
  ),
  step('wasm-build', 'Release WASM and pinned wasm-bindgen build', npmBinary, [
    'run',
    'build:wasm',
  ], { env: rustEnvironment }),
  step('typescript', 'TypeScript boundary check', npmBinary, ['run', 'typecheck']),
  step('unit-suites', 'Node and inspector browser unit suites', npmBinary, [
    'run',
    'test:unit',
  ]),
  step('accessibility-focused', 'Focused Chromium accessibility evidence', npmBinary, [
    'run',
    'test:browser',
    '--',
    'accessibility',
  ]),
  step('browser-suites', 'Full real-Chromium Phase 1 browser suites', npmBinary, [
    'run',
    'test:browser',
  ]),
])

export const deterministicReplayRounds = 2

export const deterministicReplaySteps = Object.freeze([
  step(
    'canonical-replay',
    'Checked-in canonical round-trip replay',
    cargoBinary,
    [
      'test',
      '--locked',
      '-p',
      'flow-core',
      '--test',
      'persistence_round_trip',
      'checked_in_current_fixture_is_the_exact_round_trip_contract',
      '--',
      '--exact',
    ],
    { env: rustEnvironment },
  ),
  step(
    'migration-replay',
    'Checked-in migration golden replay',
    cargoBinary,
    [
      'test',
      '--locked',
      '-p',
      'flow-core',
      '--test',
      'migration_golden',
      'older_fixture_migrates_one_pure_hop_to_the_checked_in_current_schema_boundary',
      '--',
      '--exact',
    ],
    { env: rustEnvironment },
  ),
])

export function validateTerminalBenchmarkEvidence({
  passedExists,
  blockerExists,
  report,
}) {
  if (!passedExists || blockerExists) {
    throw new Error(
      'exactly one passing recovery benchmark artifact must exist and no blocker may exist',
    )
  }
  if (report?.passed !== true || report?.blocked !== false) {
    throw new Error('recovery benchmark terminal artifact is not an unambiguous pass')
  }
}

export function resolveChildExitCode(result) {
  return Number.isInteger(result.status) ? result.status : 1
}

export function runPhaseOneGate({ spawn = spawnSync, output = process.stdout } = {}) {
  const fullStartedAt = performance.now()
  validateRequiredEvidence()
  let focusedMilliseconds = 0

  for (const gate of phaseOneSteps) {
    const result = runStep(gate, spawn, output)
    if (gate.id === 'accessibility-focused') focusedMilliseconds = result.milliseconds
    if (result.exitCode !== 0) return result.exitCode
  }
  for (let round = 1; round <= deterministicReplayRounds; round += 1) {
    output.write(`\nDeterministic replay round ${round}/${deterministicReplayRounds}\n`)
    for (const gate of deterministicReplaySteps) {
      const result = runStep(gate, spawn, output)
      if (result.exitCode !== 0) return result.exitCode
    }
  }

  const fullMilliseconds = performance.now() - fullStartedAt
  output.write(
    `\nFocused accessibility runtime: ${formatDuration(focusedMilliseconds)}\n` +
      `Full Phase 1 gate runtime: ${formatDuration(fullMilliseconds)}\n`,
  )
  return 0
}

function validateRequiredEvidence() {
  for (const path of [
    'Cargo.lock',
    'package-lock.json',
    'artifacts/provenance/phase1-dependencies.json',
    'artifacts/benchmarks/phase1-recovery.json',
    'tests/contracts/phase1-boundary.test.mjs',
    'web/tests/accessibility.browser.test.ts',
    'work/toolchains/cargo/bin/cargo',
    'work/toolchains/cargo/bin/wasm-bindgen',
    'work/playwright',
  ]) {
    if (!existsSync(resolve(projectRoot, path))) {
      throw new Error(`required Phase 1 evidence or pinned tool is missing: ${path}`)
    }
  }
  if (existsSync(resolve(projectRoot, 'artifacts/provenance/phase1-blocker.json'))) {
    throw new Error('dependency provenance blocker exists')
  }

  const passedPath = resolve(projectRoot, 'artifacts/benchmarks/phase1-recovery.json')
  const blockerPath = resolve(
    projectRoot,
    'artifacts/benchmarks/phase1-recovery-blocker.json',
  )
  validateTerminalBenchmarkEvidence({
    passedExists: existsSync(passedPath),
    blockerExists: existsSync(blockerPath),
    report: JSON.parse(readFileSync(passedPath, 'utf8')),
  })
}

function runStep(gate, spawn, output) {
  output.write(`\n[${gate.id}] ${gate.label}\n`)
  const startedAt = performance.now()
  const result = spawn(gate.command, gate.args, {
    cwd: projectRoot,
    env: gate.env ?? process.env,
    shell: false,
    stdio: 'inherit',
  })
  const milliseconds = performance.now() - startedAt
  const exitCode = resolveChildExitCode(result)
  output.write(`[${gate.id}] ${formatDuration(milliseconds)}\n`)
  if (result.error !== undefined) {
    output.write(`[${gate.id}] unable to start: ${result.error.message}\n`)
  }
  if (exitCode !== 0) {
    output.write(`[${gate.id}] failed with exit code ${exitCode}; stopping.\n`)
  }
  return { exitCode, milliseconds }
}

function step(id, label, command, args, options = {}) {
  return Object.freeze({ id, label, command, args: Object.freeze(args), ...options })
}

function formatDuration(milliseconds) {
  return `${(milliseconds / 1000).toFixed(2)}s`
}

const isMain =
  process.argv[1] !== undefined &&
  resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))

if (isMain) {
  try {
    process.exitCode = runPhaseOneGate()
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error)
    process.stderr.write(`Phase 1 gate preflight failed: ${message}\n`)
    process.exitCode = 1
  }
}
