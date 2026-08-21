#!/usr/bin/env node

import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')
const recipePath = resolve(root, 'fixtures/recovery/benchmark-200-page.recipe.json')
const outputDir = resolve(root, 'artifacts/benchmarks')
const passedPath = resolve(outputDir, 'phase1-recovery.json')
const blockedPath = resolve(outputDir, 'phase1-recovery-blocker.json')
const sourceManifestPaths = [
  'fixtures/recovery/benchmark-200-page.recipe.json',
  'Cargo.lock',
  'crates/flow-core/src/lib.rs',
  'crates/flow-core/src/store/mod.rs',
  'crates/flow-core/examples/recovery_benchmark.rs',
  'scripts/verify-recovery-benchmark.mjs',
]
const toolchain = resolveToolchain()
const sourceManifest = resolveSourceManifest()

const recipeBytes = readFileSync(recipePath)
const recipe = JSON.parse(recipeBytes.toString('utf8'))
validateRecipe(recipe)
const fixtureHash = `sha256:${createHash('sha256').update(recipeBytes).digest('hex')}`

if (process.argv.includes('--self-test')) {
  runValidatorSelfTest()
} else if (process.argv.includes('--validate')) {
  validateTerminalArtifact()
  process.stdout.write('Recovery benchmark terminal artifact is valid.\n')
} else {
  runBenchmark()
}

function runBenchmark() {
  const attempts = []
  for (const policy of recipe.candidates) {
    const measurement = runCandidate(policy)
    assertSourceManifestStable()
    attempts.push(measurement)
    if (measurement.p95Milliseconds < recipe.p95TargetMilliseconds) {
      const report = terminalReport({ attempts, selectedPolicy: policy, passed: true, blocked: false })
      writeTerminalArtifact(passedPath, blockedPath, report)
      validateTerminalArtifact()
      process.stdout.write(`${JSON.stringify(report)}\n`)
      return
    }
  }

  const report = terminalReport({ attempts, selectedPolicy: null, passed: false, blocked: true })
  writeTerminalArtifact(blockedPath, passedPath, report)
  validateTerminalArtifact()
  process.stderr.write(`${JSON.stringify(report)}\n`)
  process.exitCode = 1
}

function runCandidate(policy) {
  const stdout = execFileSync(
    toolchain.cargoBinary,
    [
      'run',
      '-p',
      'flow-core',
      '--example',
      'recovery_benchmark',
      '--release',
      '--locked',
      '--',
      recipePath,
      String(policy.transactionInterval),
      String(policy.byteInterval),
    ],
    {
      cwd: root,
      encoding: 'utf8',
      env: {
        ...process.env,
        CARGO_HOME: toolchain.cargoHome,
        RUSTUP_HOME: toolchain.rustupHome,
        PATH: `${dirname(toolchain.cargoBinary)}:${process.env.PATH ?? ''}`,
      },
      stdio: ['ignore', 'pipe', 'inherit'],
    },
  )
  const lastLine = stdout.trim().split('\n').at(-1)
  if (lastLine === undefined) {
    throw new Error('recovery benchmark example produced no measurement')
  }
  const measurement = JSON.parse(lastLine)
  validateMeasurement(measurement, policy)
  return measurement
}

function terminalReport({ attempts, selectedPolicy, passed, blocked }) {
  return {
    formatVersion: 2,
    fixtureHash,
    fixturePath: 'fixtures/recovery/benchmark-200-page.recipe.json',
    sourceManifest,
    toolchain,
    attempts,
    selectedPolicy,
    p50Milliseconds: passed ? attempts.at(-1).p50Milliseconds : null,
    p95Milliseconds: passed ? attempts.at(-1).p95Milliseconds : null,
    p95TargetMilliseconds: recipe.p95TargetMilliseconds,
    passed,
    blocked,
  }
}

function writeTerminalArtifact(targetPath, otherPath, report) {
  mkdirSync(dirname(targetPath), { recursive: true })
  rmSync(otherPath, { force: true })
  const temporaryPath = `${targetPath}.tmp-${process.pid}`
  writeFileSync(temporaryPath, `${JSON.stringify(report, null, 2)}\n`, { encoding: 'utf8', mode: 0o644 })
  renameSync(temporaryPath, targetPath)
}

function validateRecipe(value) {
  const expectedVocabulary = [
    'FlowPDF',
    'document',
    'contract',
    'revision',
    'semantic',
    'paragraph',
    'layout',
    'anchor',
    'field',
    'recovery',
    'audit',
    'deterministic',
    'документ',
    'угода',
    'сторона',
    'підпис',
  ]
  const expectedCandidates = [
    [100, 4 * 1024 * 1024],
    [50, 2 * 1024 * 1024],
    [25, 1024 * 1024],
    [10, 1024 * 1024],
  ]
  if (
    value?.formatVersion !== 2 ||
    value.name !== 'phase1-recovery-200-page-equivalent' ||
    value.pages !== 200 ||
    value.transactions !== 1000 ||
    value.backgroundMutationUtf8Bytes !== 32 ||
    value.pageEquivalent?.paragraphsPerPage !== 5 ||
    value.pageEquivalent?.wordsPerParagraph !== 80 ||
    value.pageEquivalent?.minimumUtf8BytesPerPage !== 3000 ||
    JSON.stringify(value.pageEquivalent?.vocabulary) !== JSON.stringify(expectedVocabulary) ||
    value.pages * value.pageEquivalent.paragraphsPerPage !== value.transactions ||
    value.warmups !== 3 ||
    value.measurements !== 20 ||
    value.p95TargetMilliseconds !== 2000 ||
    !Array.isArray(value.candidates) ||
    value.candidates.length !== expectedCandidates.length
  ) {
    throw new Error('benchmark recipe does not match the locked Phase 1 workload')
  }
  value.candidates.forEach((candidate, index) => {
    const [transactions, bytes] = expectedCandidates[index]
    if (candidate.transactionInterval !== transactions || candidate.byteInterval !== bytes) {
      throw new Error('benchmark recipe candidate order or cadence is not approved')
    }
  })
}

function generatedParagraph(sequence) {
  const { paragraphsPerPage, wordsPerParagraph, vocabulary } = recipe.pageEquivalent
  const page = Math.floor((sequence - 1) / paragraphsPerPage) + 1
  const paragraphOnPage = ((sequence - 1) % paragraphsPerPage) + 1
  const words = [`page-${String(page).padStart(3, '0')}`, `paragraph-${String(paragraphOnPage).padStart(2, '0')}`]
  for (let wordIndex = 2; wordIndex < wordsPerParagraph; wordIndex += 1) {
    const vocabularyIndex = (sequence + wordIndex) % vocabulary.length
    const suffix = wordIndex + 1 === wordsPerParagraph ? '.' : ''
    words.push(`${vocabulary[vocabularyIndex]}${suffix}`)
  }
  return words.join(' ')
}

function expectedWorkloadProof() {
  const { paragraphsPerPage, wordsPerParagraph, minimumUtf8BytesPerPage } = recipe.pageEquivalent
  const generatedParagraphCount = recipe.pages * paragraphsPerPage
  let generatedUtf8Bytes = 0
  let pageUtf8Bytes = 0
  for (let sequence = 1; sequence <= generatedParagraphCount; sequence += 1) {
    const paragraphBytes = Buffer.byteLength(generatedParagraph(sequence), 'utf8')
    generatedUtf8Bytes += paragraphBytes
    pageUtf8Bytes += paragraphBytes
    if (sequence % paragraphsPerPage === 0) {
      if (pageUtf8Bytes < minimumUtf8BytesPerPage) {
        throw new Error('locked page-equivalent recipe does not meet its semantic byte floor')
      }
      pageUtf8Bytes = 0
    }
  }
  return {
    pageEquivalentCount: recipe.pages,
    paragraphsPerPage,
    wordsPerParagraph,
    generatedParagraphCount,
    generatedWordCount: generatedParagraphCount * wordsPerParagraph,
    generatedUtf8Bytes,
    minimumUtf8BytesPerPage,
    initialContentNodeCount: 3,
    finalContentNodeCount: 3 + generatedParagraphCount,
    finalRevision: 1 + recipe.transactions,
    historyEntryCount: recipe.transactions,
    historyCursor: recipe.transactions,
    plannedTransactionCount: recipe.transactions,
  }
}

function validateMeasurement(measurement, policy) {
  const expectedProof = expectedWorkloadProof()
  const proof = measurement?.workloadProof
  const durations = measurement?.durationsMilliseconds
  if (
    measurement?.fixtureHash?.startsWith('blake3:') !== true ||
    measurement.fixtureName !== recipe.name ||
    measurement.pages !== recipe.pages ||
    measurement.transactions !== recipe.transactions ||
    measurement.warmups !== recipe.warmups ||
    measurement.measurements !== recipe.measurements ||
    measurement.p95TargetMilliseconds !== recipe.p95TargetMilliseconds ||
    measurement.policy?.transactionInterval !== policy.transactionInterval ||
    measurement.policy?.byteInterval !== policy.byteInterval ||
    proof?.semanticPayloadHash?.startsWith('blake3:') !== true ||
    Object.entries(expectedProof).some(([key, value]) => proof[key] !== value) ||
    !Array.isArray(durations) ||
    durations.length !== recipe.measurements
  ) {
    throw new Error('recovery benchmark returned a malformed measurement')
  }
  const { p50Milliseconds, p95Milliseconds } = recomputePercentiles(durations)
  if (
    measurement.p50Milliseconds !== p50Milliseconds ||
    measurement.p95Milliseconds !== p95Milliseconds
  ) {
    throw new Error('recovery benchmark percentile claims do not match the measured durations')
  }
}

function validateTerminalArtifact() {
  const present = [passedPath, blockedPath].filter(existsSync)
  if (present.length !== 1) {
    throw new Error('exactly one recovery benchmark terminal artifact must exist')
  }
  const report = JSON.parse(readFileSync(present[0], 'utf8'))
  validateReport(report)
  const expectedPath = report.passed ? passedPath : blockedPath
  if (present[0] !== expectedPath) {
    throw new Error('recovery benchmark result is stored under the wrong terminal artifact name')
  }
  return report
}

function validateReport(report) {
  if (
    report?.formatVersion !== 2 ||
    report.fixtureHash !== fixtureHash ||
    report.fixturePath !== 'fixtures/recovery/benchmark-200-page.recipe.json' ||
    JSON.stringify(report.sourceManifest) !== JSON.stringify(resolveSourceManifest()) ||
    !Array.isArray(report.attempts) ||
    report.attempts.length < 1 ||
    report.attempts.length > recipe.candidates.length ||
    report.p95TargetMilliseconds !== recipe.p95TargetMilliseconds ||
    JSON.stringify(report.toolchain) !== JSON.stringify(toolchain) ||
    typeof report.passed !== 'boolean' ||
    typeof report.blocked !== 'boolean' ||
    report.passed === report.blocked
  ) {
    throw new Error('recovery benchmark terminal artifact is malformed')
  }
  report.attempts.forEach((attempt, index) => validateMeasurement(attempt, recipe.candidates[index]))
  const benchmarkFixtureHash = report.attempts[0].fixtureHash
  if (report.attempts.some((attempt) => attempt.fixtureHash !== benchmarkFixtureHash)) {
    throw new Error('recovery benchmark attempts were measured against different fixture bytes')
  }
  const workloadProof = JSON.stringify(report.attempts[0].workloadProof)
  if (report.attempts.some((attempt) => JSON.stringify(attempt.workloadProof) !== workloadProof)) {
    throw new Error('recovery benchmark attempts did not use identical semantic workloads')
  }
  if (report.passed) {
    const last = report.attempts.at(-1)
    if (
      last.p95Milliseconds >= recipe.p95TargetMilliseconds ||
      report.attempts.slice(0, -1).some((attempt) => attempt.p95Milliseconds < recipe.p95TargetMilliseconds) ||
      JSON.stringify(report.selectedPolicy) !== JSON.stringify(last.policy) ||
      report.p50Milliseconds !== last.p50Milliseconds ||
      report.p95Milliseconds !== last.p95Milliseconds
    ) {
      throw new Error('passing report must select an approved policy below the p95 threshold')
    }
  } else if (
    report.attempts.length !== recipe.candidates.length ||
    report.attempts.some((attempt) => attempt.p95Milliseconds < recipe.p95TargetMilliseconds) ||
    report.selectedPolicy !== null ||
    report.p50Milliseconds !== null ||
    report.p95Milliseconds !== null
  ) {
    throw new Error('blocked report must include every failed approved candidate and no selected policy')
  }
}

// Match the Rust benchmark's documented nearest-rank algorithm exactly:
// sort ascending, then select ceil(sample_count * percentile) - 1.
function recomputePercentiles(durations) {
  if (durations.some((duration) => !Number.isFinite(duration) || duration < 0)) {
    throw new Error('recovery benchmark durations must be finite and nonnegative')
  }
  const ordered = [...durations].sort((left, right) => left - right)
  return {
    p50Milliseconds: nearestRankPercentile(ordered, 0.5),
    p95Milliseconds: nearestRankPercentile(ordered, 0.95),
  }
}

function nearestRankPercentile(sorted, percentile) {
  const index = Math.max(0, Math.ceil(sorted.length * percentile) - 1)
  return sorted[index]
}

function resolveSourceManifest() {
  const files = sourceManifestPaths.map((path) => {
    const bytes = readFileSync(resolve(root, path))
    return {
      path,
      sha256: `sha256:${createHash('sha256').update(bytes).digest('hex')}`,
    }
  })
  const manifestBytes = files.map(({ path, sha256 }) => `${path}\0${sha256}\n`).join('')
  return {
    formatVersion: 1,
    algorithm: 'sha256',
    files,
    digest: `sha256:${createHash('sha256').update(manifestBytes, 'utf8').digest('hex')}`,
  }
}

function assertSourceManifestStable() {
  if (JSON.stringify(sourceManifest) !== JSON.stringify(resolveSourceManifest())) {
    throw new Error('benchmark or recovery sources changed while the benchmark was running')
  }
}

function runValidatorSelfTest() {
  const report = validateTerminalArtifact()
  const adversarialCases = [
    ['stale source manifest', (candidate) => { candidate.sourceManifest.files[0].sha256 = 'sha256:forged' }],
    ['forged p50', (candidate) => { candidate.attempts[0].p50Milliseconds += 1 }],
    ['forged terminal p95', (candidate) => { candidate.p95Milliseconds += 1 }],
    ['negative duration', (candidate) => { candidate.attempts[0].durationsMilliseconds[0] = -1 }],
    ['non-finite duration', (candidate) => { candidate.attempts[0].durationsMilliseconds[0] = Number.POSITIVE_INFINITY }],
    ['durations detached from metrics', (candidate) => { candidate.attempts[0].durationsMilliseconds.fill(0) }],
  ]
  for (const [name, mutate] of adversarialCases) {
    const candidate = structuredClone(report)
    mutate(candidate)
    let rejected = false
    try {
      validateReport(candidate)
    } catch {
      rejected = true
    }
    if (!rejected) {
      throw new Error(`validator self-test accepted adversarial case: ${name}`)
    }
  }
  process.stdout.write(`Recovery benchmark validator rejected ${adversarialCases.length} adversarial cases.\n`)
}

function resolveToolchain() {
  const cargoHome = resolve(process.env.CARGO_HOME ?? join(root, 'work/toolchains/cargo'))
  const rustupHome = resolve(process.env.RUSTUP_HOME ?? join(root, 'work/toolchains/rustup'))
  const cargoBinary = resolve(process.env.CARGO ?? join(cargoHome, 'bin/cargo'))
  const rustcBinary = resolve(process.env.RUSTC ?? join(cargoHome, 'bin/rustc'))
  if (!existsSync(cargoBinary) || !existsSync(rustcBinary)) {
    throw new Error(
      `workspace-local Cargo/Rustc is unavailable at ${cargoBinary} / ${rustcBinary}; set CARGO, RUSTC, or CARGO_HOME explicitly`,
    )
  }
  const commandOptions = {
    cwd: root,
    encoding: 'utf8',
    env: {
      ...process.env,
      CARGO_HOME: cargoHome,
      RUSTUP_HOME: rustupHome,
      PATH: `${dirname(cargoBinary)}:${process.env.PATH ?? ''}`,
    },
  }
  const cargoVersion = execFileSync(cargoBinary, ['--version'], commandOptions).trim()
  const rustcVersion = execFileSync(rustcBinary, ['--version'], commandOptions).trim()
  return {
    node: process.version,
    platform: process.platform,
    architecture: process.arch,
    cargoBinary: resolve(root, cargoBinary).replace(`${root}/`, ''),
    rustcBinary: resolve(root, rustcBinary).replace(`${root}/`, ''),
    cargoHome: resolve(root, cargoHome).replace(`${root}/`, ''),
    rustupHome: resolve(root, rustupHome).replace(`${root}/`, ''),
    cargoVersion,
    rustcVersion,
    benchmark: 'flow-core native release',
    browser: 'not-applicable: Rust recovery benchmark; browser path is verified separately',
  }
}
