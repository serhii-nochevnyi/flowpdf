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
const toolchain = resolveToolchain()

const recipeBytes = readFileSync(recipePath)
const recipe = JSON.parse(recipeBytes.toString('utf8'))
validateRecipe(recipe)
const fixtureHash = `sha256:${createHash('sha256').update(recipeBytes).digest('hex')}`

if (process.argv.includes('--validate')) {
  validateTerminalArtifact()
  process.stdout.write('Recovery benchmark terminal artifact is valid.\n')
} else {
  runBenchmark()
}

function runBenchmark() {
  const attempts = []
  for (const policy of recipe.candidates) {
    const measurement = runCandidate(policy)
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
    formatVersion: 1,
    fixtureHash,
    fixturePath: 'fixtures/recovery/benchmark-200-page.recipe.json',
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
    !Array.isArray(measurement.durationsMilliseconds) ||
    measurement.durationsMilliseconds.length !== recipe.measurements ||
    !Number.isFinite(measurement.p50Milliseconds) ||
    !Number.isFinite(measurement.p95Milliseconds)
  ) {
    throw new Error('recovery benchmark returned a malformed measurement')
  }
}

function validateTerminalArtifact() {
  const present = [passedPath, blockedPath].filter(existsSync)
  if (present.length !== 1) {
    throw new Error('exactly one recovery benchmark terminal artifact must exist')
  }
  const report = JSON.parse(readFileSync(present[0], 'utf8'))
  if (
    report?.formatVersion !== 1 ||
    report.fixtureHash !== fixtureHash ||
    !Array.isArray(report.attempts) ||
    report.attempts.length < 1 ||
    report.p95TargetMilliseconds !== recipe.p95TargetMilliseconds ||
    JSON.stringify(report.toolchain) !== JSON.stringify(toolchain) ||
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
      JSON.stringify(report.selectedPolicy) !== JSON.stringify(last.policy)
    ) {
      throw new Error('passing report must select an approved policy below the p95 threshold')
    }
  } else if (report.attempts.length !== recipe.candidates.length || report.selectedPolicy !== null) {
    throw new Error('blocked report must include every approved candidate and no selected policy')
  }
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
