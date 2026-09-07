#!/usr/bin/env node

import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import {
  appendFileSync,
  cpSync,
  existsSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  readdirSync,
  renameSync,
  rmSync,
  statSync,
  writeFileSync,
} from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { isDeepStrictEqual } from 'node:util'
import { gzipSync } from 'node:zlib'

import { assertSupportedNodeVersion } from './node-version.mjs'
import {
  validateWasmBindgenIntegrity,
  validateWasmBindgenVersion,
} from './verify-wasm-bindgen-tool.mjs'

assertSupportedNodeVersion()

export const WASM_SIZE_LIMITS = Object.freeze({
  rawDeltaBytes: 512 * 1024,
  gzipDeltaBytes: 160 * 1024,
})

const projectRoot = resolve(import.meta.dirname, '..')
const artifactPath = resolve(projectRoot, 'artifacts/benchmarks/phase2-wasm-size.json')
const dependencyReportPath = resolve(projectRoot, 'artifacts/provenance/phase2-dependencies.json')
const target = 'wasm32-unknown-unknown'
const rustFlags = '-C debuginfo=0'
const maximumArtifactBytes = 1024 * 1024
const acceptedLockIntentDigest = '27c9a5f2f41d0148ee1ff8e277ee1990f48135f367f555d709daed7604666bae'
const acceptedIcuChecksum = '82d07aafccd67af15d02512a6adf5896fbc5ed00f2e99b471d2efa14016db3db'
const probeManifestAppend = `

[features]
default = []
icu-segmenter = ["dep:icu_segmenter"]

[dependencies.icu_segmenter]
workspace = true
optional = true
`
const probeSourceAppend = `

// Measurement-only export appended in an isolated workspace. Its input keeps
// ICU's compiled grapheme data runtime-reachable in the candidate binary.
#[cfg(feature = "icu-segmenter")]
#[wasm_bindgen]
pub fn icu_grapheme_probe(input: &str) -> u32 {
    icu_segmenter::GraphemeClusterSegmenter::new()
        .segment_str(input)
        .count() as u32
}
`
const fixedSourcePaths = [
  'Cargo.toml',
  'Cargo.lock',
  'rust-toolchain.toml',
  'crates/flow-core/Cargo.toml',
  'crates/flow-wasm/Cargo.toml',
  'fixtures/flowdoc/older.json',
  'config/dependency-provenance.json',
  'scripts/verify-wasm-bindgen-tool.mjs',
  'scripts/verify-wasm-size.mjs',
]
const compression = Object.freeze({
  algorithm: 'gzip',
  level: 9,
  mtime: 0,
  normalizedHeader: { mtimeBytes: [0, 0, 0, 0], operatingSystem: 255 },
})

export function validateWasmSizeReport(report, expectedContext) {
  if (
    report?.formatVersion !== 1 ||
    report.status !== 'passed' ||
    report.passed !== true ||
    report.blocked !== false
  ) {
    throw new Error('WASM size report does not describe one passing terminal result')
  }
  if (
    report.sourceManifest?.formatVersion !== 1 ||
    report.sourceManifest.algorithm !== 'sha256' ||
    !Array.isArray(report.sourceManifest.files) ||
    report.sourceManifest.files.length === 0 ||
    !isSha256(report.sourceManifest.digest) ||
    report.sourceManifest.files.some(
      (entry) =>
        typeof entry?.path !== 'string' ||
        entry.path.length === 0 ||
        !isSha256(entry.sha256),
    )
  ) {
    throw new Error('WASM size report source manifest is malformed')
  }
  if (
    typeof report.toolchain?.cargoVersion !== 'string' ||
    typeof report.toolchain?.rustcVersion !== 'string' ||
    typeof report.toolchain?.wasmBindgenVersion !== 'string' ||
    !isSha256(report.toolchain?.wasmBindgenBinarySha256) ||
    typeof report.toolchain?.wasmBindgenReceiptTarget !== 'string' ||
    report.toolchain.wasmBindgenReceiptTarget.length === 0 ||
    report.toolchain?.wasmBindgenProvenanceIdentity?.name !== 'wasm-bindgen-cli' ||
    report.toolchain.wasmBindgenProvenanceIdentity.version !== '0.2.108' ||
    report.toolchain.wasmBindgenProvenanceIdentity.kind !== 'tool' ||
    typeof report.toolchain?.nodeVersion !== 'string' ||
    typeof report.toolchain?.zlibVersion !== 'string' ||
    report.toolchain.target !== target
  ) {
    throw new Error('WASM size report toolchain identity is malformed')
  }
  if (
    report.buildConfiguration?.profile !== 'release' ||
    report.buildConfiguration.locked !== true ||
    report.buildConfiguration.target !== target ||
    report.buildConfiguration.package !== 'flow-wasm' ||
    !Array.isArray(report.buildConfiguration.baselineFeatures) ||
    !Array.isArray(report.buildConfiguration.candidateFeatures) ||
    report.buildConfiguration.rustFlags !== rustFlags ||
    report.buildConfiguration.bindgenTarget !== 'web'
  ) {
    throw new Error('WASM size report build configuration is malformed')
  }
  if (
    report.compression?.algorithm !== 'gzip' ||
    report.compression.level !== 9 ||
    report.compression.mtime !== 0
  ) {
    throw new Error('WASM size report compression settings are not deterministic')
  }
  if (!isDeepStrictEqual(report.limits, WASM_SIZE_LIMITS)) {
    throw new Error('WASM size report budgets do not match the Phase 2 limits')
  }

  validateMeasurement(report.baseline, 'phase1-compatible')
  validateMeasurement(report.candidate, 'icu-compiled-data')
  if (
    !Number.isSafeInteger(report.delta?.rawBytes) ||
    !Number.isSafeInteger(report.delta?.gzipBytes) ||
    report.delta.rawBytes !== report.candidate.rawBytes - report.baseline.rawBytes ||
    report.delta.gzipBytes !== report.candidate.gzipBytes - report.baseline.gzipBytes
  ) {
    throw new Error('WASM size report delta does not match its measured totals')
  }
  if (report.delta.rawBytes > WASM_SIZE_LIMITS.rawDeltaBytes) {
    throw new Error('WASM raw delta exceeds the 512 KiB budget')
  }
  if (report.delta.gzipBytes > WASM_SIZE_LIMITS.gzipDeltaBytes) {
    throw new Error('WASM gzip delta exceeds the 160 KiB budget')
  }

  if (expectedContext !== undefined) {
    for (const key of [
      'sourceManifest',
      'toolchain',
      'buildConfiguration',
      'compression',
      'baseline',
      'candidate',
    ]) {
      if (!isDeepStrictEqual(report[key], expectedContext[key])) {
        throw new Error(`WASM size report ${key} does not match fresh build evidence`)
      }
    }
  }
}

function validateMeasurement(measurement, expectedLabel) {
  if (
    measurement?.label !== expectedLabel ||
    !Number.isSafeInteger(measurement.rawBytes) ||
    measurement.rawBytes <= 0 ||
    !Number.isSafeInteger(measurement.gzipBytes) ||
    measurement.gzipBytes <= 0 ||
    !isSha256(measurement.sha256) ||
    ('gzipSha256' in measurement && !isSha256(measurement.gzipSha256))
  ) {
    throw new Error(`WASM size report ${expectedLabel} measurement is malformed`)
  }
}

function isSha256(value) {
  return typeof value === 'string' && /^sha256:[0-9a-f]{64}$/.test(value)
}

function main() {
  const record = process.argv.includes('--record')
  const unknownArguments = process.argv.slice(2).filter((argument) => argument !== '--record')
  if (unknownArguments.length > 0) {
    throw new Error(`unsupported argument ${unknownArguments[0]}`)
  }

  const icuIdentity = validateAcceptedDependency()
  const verifiedWasmBindgen = materializeVerifiedWasmBindgen(projectRoot)
  try {
    runSizeGate(record, verifiedWasmBindgen, icuIdentity)
  } finally {
    verifiedWasmBindgen.cleanup()
  }
}

function runSizeGate(record, verifiedWasmBindgen, icuIdentity) {
  const sourceManifest = resolveSourceManifest({ verifiedWasmBindgen, icuIdentity })
  const runtimeToolchain = resolveToolchain(verifiedWasmBindgen)
  const toolchain = {
    cargoVersion: runtimeToolchain.cargoVersion,
    rustcVersion: runtimeToolchain.rustcVersion,
    wasmBindgenVersion: runtimeToolchain.wasmBindgenVersion,
    wasmBindgenBinarySha256: verifiedWasmBindgen.binarySha256,
    wasmBindgenReceiptTarget: verifiedWasmBindgen.receiptTarget,
    wasmBindgenProvenanceIdentity: verifiedWasmBindgen.provenanceIdentity,
    nodeVersion: process.version,
    zlibVersion: process.versions.zlib,
    target,
  }
  const buildConfiguration = {
    profile: 'release',
    locked: true,
    frozen: true,
    target,
    package: 'flow-wasm',
    baselineFeatures: [],
    candidateFeatures: ['icu-segmenter'],
    rustFlags,
    bindgenTarget: 'web',
    commonCargoArguments: [
      'build',
      '--package',
      'flow-wasm',
      '--target',
      target,
      '--release',
      '--frozen',
    ],
    commonBindgenArguments: ['--target', 'web', '--out-name', 'flow_wasm'],
    measuredOutput: 'flow_wasm_bg.wasm',
    onlyVariant: 'icu-segmenter feature activation',
  }
  const { baseline, candidate } = measureIsolatedBuilds(runtimeToolchain)
  if (
    !isDeepStrictEqual(
      resolveSourceManifest({ verifiedWasmBindgen, icuIdentity: validateAcceptedDependency() }),
      sourceManifest,
    )
  ) {
    throw new Error('WASM size source inputs changed while builds were running')
  }

  const report = {
    formatVersion: 1,
    status: 'passed',
    sourceManifest,
    toolchain,
    buildConfiguration,
    compression,
    limits: { ...WASM_SIZE_LIMITS },
    baseline,
    candidate,
    delta: {
      rawBytes: candidate.rawBytes - baseline.rawBytes,
      gzipBytes: candidate.gzipBytes - baseline.gzipBytes,
    },
    passed: true,
    blocked: false,
  }
  validateWasmSizeReport(report, expectedContext(report))

  if (record) {
    writeArtifact(report)
  } else {
    const checked = readBoundedJson(artifactPath, maximumArtifactBytes, 'WASM size artifact')
    validateWasmSizeReport(checked, expectedContext(report))
    if (!isDeepStrictEqual(checked, report)) {
      throw new Error('checked WASM size artifact is stale or contains forged claims')
    }
  }
  process.stdout.write(
    `WASM size gate passed: raw ${signed(report.delta.rawBytes)} bytes, gzip ${signed(report.delta.gzipBytes)} bytes.\n`,
  )
}

function expectedContext(report) {
  return {
    sourceManifest: report.sourceManifest,
    toolchain: report.toolchain,
    buildConfiguration: report.buildConfiguration,
    compression: report.compression,
    baseline: report.baseline,
    candidate: report.candidate,
  }
}

function measureIsolatedBuilds(toolchain) {
  const temporaryRoot = mkdtempSync(join(tmpdir(), 'flowpdf-wasm-size-'))
  try {
    const workspace = join(temporaryRoot, 'workspace')
    copyMeasurementWorkspace(workspace)
    appendProbe(workspace)
    prepareMeasurementLock(workspace, toolchain)
    const baseline = buildVariant({
      workspace,
      temporaryRoot,
      label: 'phase1-compatible',
      features: [],
      toolchain,
    })
    const candidate = buildVariant({
      workspace,
      temporaryRoot,
      label: 'icu-compiled-data',
      features: ['icu-segmenter'],
      toolchain,
    })
    return { baseline, candidate }
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true, maxRetries: 2 })
  }
}

function prepareMeasurementLock(workspace, toolchain) {
  runPinned(toolchain.cargoBinary, ['generate-lockfile', '--offline'], {
    cwd: workspace,
    env: toolchainEnvironment(toolchain),
  })
  const acceptedLock = readFileSync(resolve(projectRoot, 'Cargo.lock'), 'utf8')
  const measurementLock = readFileSync(resolve(workspace, 'Cargo.lock'), 'utf8')
  if (!isDeepStrictEqual(registryPackageBlocks(measurementLock), registryPackageBlocks(acceptedLock))) {
    throw new Error('measurement lock changed a registry package from the accepted Cargo.lock')
  }
  const flowWasmBlock = measurementLock
    .split('[[package]]')
    .find((block) => /\nname = "flow-wasm"\n/.test(`\n${block}`))
  if (flowWasmBlock === undefined || !flowWasmBlock.includes('"icu_segmenter"')) {
    throw new Error('measurement lock does not bind the isolated ICU probe dependency')
  }
}

function registryPackageBlocks(lock) {
  return lock
    .split('[[package]]')
    .map((block) => block.trim())
    .filter((block) => block.includes('source = "registry+'))
    .sort()
}

function copyMeasurementWorkspace(workspace) {
  mkdirSync(workspace, { recursive: true })
  for (const path of ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml']) {
    cpSync(resolve(projectRoot, path), resolve(workspace, path))
  }
  cpSync(resolve(projectRoot, 'crates'), resolve(workspace, 'crates'), { recursive: true })
  cpSync(resolve(projectRoot, 'fixtures/flowdoc'), resolve(workspace, 'fixtures/flowdoc'), {
    recursive: true,
  })
}

function appendProbe(workspace) {
  const manifestPath = resolve(workspace, 'crates/flow-wasm/Cargo.toml')
  const sourcePath = resolve(workspace, 'crates/flow-wasm/src/lib.rs')
  const manifest = readFileSync(manifestPath, 'utf8')
  const source = readFileSync(sourcePath, 'utf8')
  if (manifest.includes('icu-segmenter') || source.includes('icu_grapheme_probe')) {
    throw new Error('measurement probe collides with checked production source')
  }
  appendFileSync(manifestPath, probeManifestAppend, 'utf8')
  appendFileSync(sourcePath, probeSourceAppend, 'utf8')
}

function buildVariant({ workspace, temporaryRoot, label, features, toolchain }) {
  const variant = label === 'phase1-compatible' ? 'baseline' : 'candidate'
  const targetDirectory = resolve(temporaryRoot, `${variant}-target`)
  const bindgenDirectory = resolve(temporaryRoot, `${variant}-bindgen`)
  const cargoArguments = [
    'build',
    '--package',
    'flow-wasm',
    '--target',
    target,
    '--release',
    '--frozen',
  ]
  if (features.length > 0) cargoArguments.push('--features', features.join(','))
  process.stdout.write(`Building ${variant} release WASM in an isolated workspace...\n`)
  runPinned(toolchain.cargoBinary, [...cargoArguments, '--target-dir', targetDirectory], {
    cwd: workspace,
    env: toolchainEnvironment(toolchain),
  })

  mkdirSync(bindgenDirectory, { recursive: true })
  const inputPath = resolve(targetDirectory, target, 'release/flow_wasm.wasm')
  runPinned(
    toolchain.wasmBindgenBinary,
    [
      inputPath,
      '--target',
      'web',
      '--out-dir',
      bindgenDirectory,
      '--out-name',
      'flow_wasm',
    ],
    { cwd: workspace, env: toolchainEnvironment(toolchain) },
  )
  const wasmBytes = readFileSync(resolve(bindgenDirectory, 'flow_wasm_bg.wasm'))
  const gzipBytes = deterministicGzip(wasmBytes)
  return {
    label,
    rawBytes: wasmBytes.byteLength,
    gzipBytes: gzipBytes.byteLength,
    sha256: sha256(wasmBytes),
    gzipSha256: sha256(gzipBytes),
  }
}

function deterministicGzip(bytes) {
  const compressed = gzipSync(bytes, { level: compression.level, mtime: compression.mtime })
  if (compressed.byteLength < 10 || compressed[0] !== 0x1f || compressed[1] !== 0x8b) {
    throw new Error('Node did not return a valid gzip stream')
  }
  compressed.fill(0, 4, 8)
  compressed[9] = compression.normalizedHeader.operatingSystem
  return compressed
}

function resolveSourceManifest({ verifiedWasmBindgen, icuIdentity }) {
  const paths = [...fixedSourcePaths]
  for (const directory of ['crates/flow-core/src', 'crates/flow-wasm/src']) {
    paths.push(...recursiveFiles(resolve(projectRoot, directory)).map((path) => relative(projectRoot, path)))
  }
  const files = [...new Set(paths)].sort().map((path) => ({
    path,
    sha256: sha256(readFileSync(resolve(projectRoot, path))),
  }))
  files.push(
    { path: '$measurement/flow-wasm-Cargo.append.toml', sha256: sha256(probeManifestAppend) },
    { path: '$measurement/flow-wasm-src.append.rs', sha256: sha256(probeSourceAppend) },
    { path: '$provenance/icu-segmenter.json', sha256: sha256(`${JSON.stringify(icuIdentity)}\n`) },
    {
      path: '$provenance/wasm-bindgen-cli.json',
      sha256: sha256(`${JSON.stringify({
        ...verifiedWasmBindgen.provenanceIdentity,
        binarySha256: verifiedWasmBindgen.binarySha256,
        receiptTarget: verifiedWasmBindgen.receiptTarget,
      })}\n`),
    },
  )
  files.sort((left, right) => left.path.localeCompare(right.path))
  return {
    formatVersion: 1,
    algorithm: 'sha256',
    files,
    digest: sha256(`${JSON.stringify(files)}\n`),
  }
}

function recursiveFiles(directory) {
  const files = []
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name)
    if (entry.isDirectory()) files.push(...recursiveFiles(path))
    else if (entry.isFile()) files.push(path)
    else throw new Error(`unsupported source entry ${relative(projectRoot, path)}`)
  }
  return files
}

export function materializeVerifiedWasmBindgen(root) {
  const cargoHome = resolve(root, 'work/toolchains/cargo')
  const approvedConfig = readBoundedJson(
    resolve(root, 'config/dependency-provenance.json'),
    maximumArtifactBytes,
    'dependency approval config',
  )
  const provenanceReport = readBoundedJson(
    resolve(root, 'artifacts/provenance/phase1-dependencies.json'),
    maximumArtifactBytes,
    'Phase 1 dependency provenance',
  )
  const receipt = readBoundedJson(
    resolve(cargoHome, '.crates2.json'),
    maximumArtifactBytes,
    'Cargo installation receipt',
  )
  const approved = approvedConfig.crates?.find((entry) => entry.name === 'wasm-bindgen-cli')
  const installedBinary = resolve(cargoHome, 'bin/wasm-bindgen')
  const binaryBytes = readBoundedBytes(installedBinary, 16 * 1024 * 1024, 'wasm-bindgen binary')

  validateWasmBindgenIntegrity({
    approved,
    report: provenanceReport,
    receipt,
    binaryBytes,
  })

  const receiptKey = `wasm-bindgen-cli ${approved.version} (registry+https://github.com/rust-lang/crates.io-index)`
  const receiptTarget = receipt.installs[receiptKey].target
  const trustedDirectory = mkdtempSync(join(tmpdir(), 'flowpdf-wasm-size-bindgen-'))
  const trustedBinary = resolve(trustedDirectory, 'wasm-bindgen')
  try {
    writeFileSync(trustedBinary, binaryBytes, { mode: 0o700 })
    const versionOutput = runPinned(trustedBinary, ['--version'], {
      cwd: root,
      env: process.env,
    })
    validateWasmBindgenVersion(approved, versionOutput)
  } catch (error) {
    rmSync(trustedDirectory, { recursive: true, force: true })
    throw error
  }

  return {
    binaryPath: trustedBinary,
    binarySha256: sha256(binaryBytes),
    receiptTarget,
    provenanceIdentity: {
      name: approved.name,
      version: approved.version,
      kind: approved.kind,
    },
    version: `wasm-bindgen ${approved.version}`,
    cleanup: () => rmSync(trustedDirectory, { recursive: true, force: true }),
  }
}

function resolveToolchain(verifiedWasmBindgen) {
  const cargoHome = resolve(projectRoot, 'work/toolchains/cargo')
  const rustupHome = resolve(projectRoot, 'work/toolchains/rustup')
  const binaryDirectory = resolve(cargoHome, 'bin')
  const cargoBinary = resolve(binaryDirectory, 'cargo')
  const rustcBinary = resolve(binaryDirectory, 'rustc')
  const wasmBindgenBinary = verifiedWasmBindgen.binaryPath
  for (const path of [cargoBinary, rustcBinary, wasmBindgenBinary]) {
    if (!existsSync(path)) throw new Error('a pinned Rust/WASM tool is missing')
  }
  const partial = { cargoHome, rustupHome, cargoBinary, rustcBinary, wasmBindgenBinary }
  const env = toolchainEnvironment(partial)
  const cargoVersion = runPinned(cargoBinary, ['--version'], { cwd: projectRoot, env }).trim()
  const rustcVersion = runPinned(rustcBinary, ['--version'], { cwd: projectRoot, env }).trim()
  const wasmBindgenVersion = verifiedWasmBindgen.version
  if (
    !cargoVersion.startsWith('cargo 1.97.1 ') ||
    !rustcVersion.startsWith('rustc 1.97.1 ') ||
    wasmBindgenVersion !== 'wasm-bindgen 0.2.108'
  ) {
    throw new Error('installed Rust/WASM tools do not match the pinned Phase 2 toolchain')
  }
  return {
    cargoVersion,
    rustcVersion,
    wasmBindgenVersion,
    target,
    cargoHome,
    rustupHome,
    cargoBinary,
    rustcBinary,
    wasmBindgenBinary,
  }
}

function toolchainEnvironment(toolchain) {
  return {
    ...process.env,
    CARGO_HOME: toolchain.cargoHome,
    RUSTUP_HOME: toolchain.rustupHome,
    PATH: `${dirname(toolchain.cargoBinary)}:${process.env.PATH ?? ''}`,
    CARGO_INCREMENTAL: '0',
    RUSTFLAGS: rustFlags,
    SOURCE_DATE_EPOCH: '0',
  }
}

function runPinned(binary, arguments_, { cwd, env }) {
  const result = spawnSync(binary, arguments_, {
    cwd,
    env,
    encoding: 'utf8',
    maxBuffer: 16 * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  if (result.error) throw result.error
  if (result.status !== 0) {
    const diagnostic = `${result.stderr ?? ''}\n${result.stdout ?? ''}`.trim().split('\n').slice(-30).join('\n')
    throw new Error(`pinned command failed (${result.status}): ${diagnostic}`)
  }
  return result.stdout ?? ''
}

function validateAcceptedDependency() {
  const report = readBoundedJson(dependencyReportPath, maximumArtifactBytes, 'dependency provenance')
  const crate = report.crates?.find((entry) => entry.name === 'icu_segmenter')
  const lockIntent = report.lockIntent?.crates?.find((entry) => entry.name === 'icu_segmenter')
  if (
    report.schemaVersion !== 1 ||
    report.status !== 'success' ||
    report.lockIntentDigest !== acceptedLockIntentDigest ||
    crate?.version !== '2.3.0' ||
    crate.checksum !== acceptedIcuChecksum ||
    crate.yanked !== false ||
    crate.legitimacy?.verdict !== 'OK' ||
    lockIntent?.version !== '2.3.0' ||
    lockIntent.defaultFeatures !== false ||
    !isDeepStrictEqual(lockIntent.features, ['compiled_data'])
  ) {
    throw new Error('ICU4X dependency does not match the accepted Plan 02-01 provenance')
  }
  const lock = readFileSync(resolve(projectRoot, 'Cargo.lock'), 'utf8')
  const packageBlock = lock
    .split('[[package]]')
    .find((block) => /\nname = "icu_segmenter"\n/.test(`\n${block}`))
  if (
    packageBlock === undefined ||
    !packageBlock.includes('version = "2.3.0"') ||
    !packageBlock.includes(`checksum = "${acceptedIcuChecksum}"`)
  ) {
    throw new Error('Cargo.lock does not bind the accepted ICU4X package checksum')
  }
  return {
    name: 'icu_segmenter',
    version: crate.version,
    checksum: crate.checksum,
    defaultFeatures: lockIntent.defaultFeatures,
    features: [...lockIntent.features],
    legitimacy: crate.legitimacy.verdict,
    lockIntentDigest: report.lockIntentDigest,
  }
}

function readBoundedJson(path, maximumBytes, label) {
  const bytes = readBoundedBytes(path, maximumBytes, label)
  try {
    return JSON.parse(bytes.toString('utf8'))
  } catch {
    throw new Error(`${label} is not valid JSON`)
  }
}

function readBoundedBytes(path, maximumBytes, label) {
  const size = statSync(path).size
  if (size <= 0 || size > maximumBytes) throw new Error(`${label} exceeds its bounded size`)
  const bytes = readFileSync(path)
  if (bytes.byteLength !== size) throw new Error(`${label} changed while it was read`)
  return bytes
}

function writeArtifact(report) {
  mkdirSync(dirname(artifactPath), { recursive: true })
  const temporaryPath = `${artifactPath}.tmp-${process.pid}`
  writeFileSync(temporaryPath, `${JSON.stringify(report, null, 2)}\n`, { mode: 0o644 })
  renameSync(temporaryPath, artifactPath)
}

function sha256(value) {
  return `sha256:${createHash('sha256').update(value).digest('hex')}`
}

function signed(value) {
  return value >= 0 ? `+${value}` : String(value)
}

function isMainModule() {
  return process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)
}

if (isMainModule()) {
  try {
    main()
  } catch (error) {
    process.stderr.write(`WASM_SIZE_GATE_FAILED: ${error instanceof Error ? error.message : String(error)}\n`)
    process.exitCode = 1
  }
}
