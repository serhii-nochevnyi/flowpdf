import assert from 'node:assert/strict'
import { mkdtemp, readFile, readdir, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import test from 'node:test'

const EXACT_VERSION = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?$/
const CRATES_IO_SOURCE = 'registry+https://github.com/rust-lang/crates.io-index'

function invariant(condition, message) {
  if (!condition) throw new Error(message)
}

export function parseCargoPackages(source) {
  return source
    .split('[[package]]')
    .slice(1)
    .map((block) => {
      const value = (key) => block.match(new RegExp(`^${key} = "([^"]+)"$`, 'm'))?.[1]
      return {
        name: value('name'),
        version: value('version'),
        source: value('source'),
        checksum: value('checksum'),
      }
    })
}

export function parseWorkspaceDependencies(source) {
  const dependencies = []
  let foundSection = false
  let inSection = false
  for (const rawLine of source.split('\n')) {
    const line = rawLine.replace(/\s+#.*$/, '').trim()
    if (line.length === 0) continue
    if (line.startsWith('[')) {
      inSection = line === '[workspace.dependencies]'
      foundSection ||= inSection
      continue
    }
    if (!inSection) continue
    const match = line.match(/^([A-Za-z0-9_-]+)\s*=\s*(?:"([^"]+)"|\{([\s\S]*)\})$/)
    invariant(match, `Cargo.toml workspace dependency is not a supported exact registry entry: ${line}`)
    const rawVersion = match[2] ?? match[3]?.match(/(?:^|,)\s*version\s*=\s*"([^"]+)"/)?.[1]
    invariant(rawVersion?.startsWith('=') && EXACT_VERSION.test(rawVersion.slice(1)), `${match[1]}: workspace Cargo dependency is not exact`)
    dependencies.push({ name: match[1], version: rawVersion.slice(1), kind: 'dependency' })
  }
  invariant(foundSection, 'Cargo.toml workspace dependencies section is missing')
  invariant(dependencies.length > 0, 'Cargo.toml workspace dependencies section is empty')
  return dependencies
}

export function parseCargoManifestDependencies(source, workspaceDependencies) {
  const workspaceByName = new Map(workspaceDependencies.map((entry) => [entry.name, entry]))
  const dependencies = []
  let inDependencySection = false
  for (const rawLine of source.split('\n')) {
    const line = rawLine.replace(/\s+#.*$/, '').trim()
    if (line.length === 0) continue
    if (line.startsWith('[')) {
      const section = line.slice(1, -1)
      inDependencySection = /^(?:.*\.)?(?:build-|dev-)?dependencies$/.test(section)
      continue
    }
    if (!inDependencySection) continue
    const match = line.match(/^([A-Za-z0-9_-]+)(\.workspace)?\s*=\s*(.*)$/)
    invariant(match, `Cargo manifest dependency entry is malformed: ${line}`)
    const [, localName, workspaceSuffix, value] = match
    const packageName = value.match(/(?:^|[,{])\s*package\s*=\s*"([^"]+)"/)?.[1] ?? localName
    const usesWorkspace = workspaceSuffix !== undefined || /(?:^|[,{])\s*workspace\s*=\s*true(?:\s*[,}]|$)/.test(value)
    if (usesWorkspace) {
      const workspace = workspaceByName.get(packageName)
      invariant(workspace, `${packageName}: workspace dependency declaration is missing`)
      dependencies.push(workspace)
      continue
    }
    const rawVersion = value.match(/^"([^"]+)"$/)?.[1]
      ?? value.match(/(?:^|[,{])\s*version\s*=\s*"([^"]+)"/)?.[1]
    if (rawVersion === undefined && /(?:^|[,{])\s*(?:path|git)\s*=/.test(value)) continue
    invariant(rawVersion?.startsWith('=') && EXACT_VERSION.test(rawVersion.slice(1)), `${packageName}: direct Cargo dependency is not exact`)
    dependencies.push({ name: packageName, version: rawVersion.slice(1), kind: 'dependency' })
  }
  return dependencies
}

function identity(entry, ecosystem) {
  return `${ecosystem}:${entry.name}@${entry.version}:${entry.kind ?? 'dependency'}`
}

function assertExactIdentities(actual, expected, label, ecosystem) {
  const actualKeys = actual.map((entry) => identity(entry, ecosystem)).sort()
  const expectedKeys = expected.map((entry) => identity(entry, ecosystem)).sort()
  invariant(
    JSON.stringify(actualKeys) === JSON.stringify(expectedKeys),
    `${label} identity coverage differs: expected ${expectedKeys.join(', ')}, received ${actualKeys.join(', ')}`,
  )
}

export function mergeProvenanceReports(reports) {
  const merged = { status: 'success', crates: [], npm: [] }
  for (const report of reports) {
    invariant(report?.status === 'success', 'dependency provenance report is not successful')
    merged.crates.push(...(report.crates ?? []))
    merged.npm.push(...(report.npm ?? []))
  }
  for (const [ecosystem, entries] of [['crates.io', merged.crates], ['npm', merged.npm]]) {
    const identities = entries.map((entry) => identity(entry, ecosystem))
    invariant(new Set(identities).size === identities.length, `${ecosystem} provenance reports contain duplicate identities`)
  }
  return merged
}

function parseFeatureList(value, label) {
  const featureSource = value.match(/(?:^|,)\s*features\s*=\s*\[([^\]]*)\]/)?.[1]
  invariant(featureSource !== undefined, `${label}: feature list is missing`)
  return [...featureSource.matchAll(/"([^"]+)"/g)].map((match) => match[1]).sort()
}

export function verifyPhase2LockIntent(packageJson, cargoToml, cargoMemberManifests, phase2) {
  invariant(phase2?.status === 'success', 'Phase 2 dependency provenance report is not successful')
  for (const item of phase2.npm ?? []) {
    invariant(item.legitimacy?.verdict === 'OK', `${item.name}@${item.version}: npm legitimacy is not OK`)
    invariant(item.lockIntent?.version === item.version, `${item.name}@${item.version}: npm report lock intent is stale`)
    invariant(packageJson[item.lockIntent.section]?.[item.name] === item.version, `${item.name}@${item.version}: package.json differs from accepted lock intent`)
  }
  for (const item of phase2.crates ?? []) {
    invariant(item.legitimacy?.verdict === 'OK', `${item.name}@${item.version}: crate legitimacy is not OK`)
    invariant(item.lockIntent?.version === item.version, `${item.name}@${item.version}: crate report lock intent is stale`)
    const escapedName = item.name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
    const workspaceValue = cargoToml.match(new RegExp(`^${escapedName}\\s*=\\s*\\{([^\\n]+)\\}$`, 'm'))?.[1]
    invariant(workspaceValue, `${item.name}: workspace dependency declaration is missing`)
    invariant(new RegExp(`(?:^|,)\\s*version\\s*=\\s*"=${item.version.replace(/\./g, '\\.')}"(?:\\s*[,]|$)`).test(workspaceValue), `${item.name}: workspace version differs from accepted lock intent`)
    invariant(/(?:^|,)\s*default-features\s*=\s*false(?:\s*[,]?|$)/.test(workspaceValue), `${item.name}: default Cargo features must be disabled`)
    invariant(
      JSON.stringify(parseFeatureList(workspaceValue, item.name)) === JSON.stringify([...item.lockIntent.features].sort()),
      `${item.name}: Cargo feature set differs from accepted lock intent`,
    )
    invariant(item.lockIntent.defaultFeatures === false, `${item.name}: report must disable default Cargo features`)
    const memberSource = cargoMemberManifests.get(item.lockIntent.manifest)
    invariant(memberSource, `${item.name}: lock-intent manifest ${item.lockIntent.manifest} is missing`)
    invariant(new RegExp(`^${escapedName}\\.workspace\\s*=\\s*true$`, 'm').test(memberSource), `${item.name}: lock-intent manifest does not consume the workspace pin`)
  }
}

export function verifyManifestCoverage(
  packageJson,
  cargoToml,
  config,
  provenance,
  cargoMemberManifests = [],
) {
  const workspaceDependencies = parseWorkspaceDependencies(cargoToml)
  const cargoDependencies = [
    ...new Map(
      [
        ...workspaceDependencies,
        ...cargoMemberManifests.flatMap((source) =>
          parseCargoManifestDependencies(source, workspaceDependencies)),
      ].map((entry) => [identity(entry, 'crates.io'), entry]),
    ).values(),
  ]
  const npmDependencies = Object.entries({
    ...packageJson.dependencies,
    ...packageJson.devDependencies,
  }).map(([name, version]) => ({ name, version, kind: 'dependency' }))
  assertExactIdentities(
    config.crates.filter((entry) => (entry.kind ?? 'dependency') !== 'tool'),
    cargoDependencies,
    'Cargo provenance manifest',
    'crates.io',
  )
  assertExactIdentities(config.npm, npmDependencies, 'npm provenance manifest', 'npm')
  assertExactIdentities(provenance.crates, config.crates, 'Cargo provenance report', 'crates.io')
  assertExactIdentities(provenance.npm, config.npm, 'npm provenance report', 'npm')
}

export function isExpectedNpmTarball(packageName, version, value) {
  if (typeof value !== 'string') return false
  let url
  try {
    url = new URL(value)
  } catch {
    return false
  }
  const basename = packageName.split('/').at(-1)
  return (
    url.protocol === 'https:' &&
    url.hostname === 'registry.npmjs.org' &&
    url.port === '' &&
    url.username === '' &&
    url.password === '' &&
    url.search === '' &&
    url.hash === '' &&
    url.pathname === `/${packageName}/-/${basename}-${version}.tgz`
  )
}

export function verifyCargoLock(cargoLock, provenance) {
  invariant(/^version = 4$/m.test(cargoLock), 'Cargo.lock must use lock format 4')
  const packages = parseCargoPackages(cargoLock)
  invariant(packages.length > 0, 'Cargo.lock contains no packages')
  for (const pkg of packages) {
    invariant(pkg.name && pkg.version, 'Cargo.lock package identity is incomplete')
    if (!pkg.source) continue
    invariant(pkg.source === CRATES_IO_SOURCE, `${pkg.name}: non-crates.io Cargo source ${pkg.source}`)
    invariant(/^[a-f0-9]{64}$/i.test(pkg.checksum ?? ''), `${pkg.name}: registry checksum missing`)
  }
  for (const direct of provenance.crates) {
    if (direct.kind === 'tool') continue
    const match = packages.find((pkg) => pkg.name === direct.name && pkg.version === direct.version)
    invariant(match, `${direct.name}@${direct.version}: direct crate missing from Cargo.lock`)
    invariant(match.checksum === direct.checksum, `${direct.name}@${direct.version}: Cargo checksum differs from provenance`)
  }
}

export function verifyNpmLock(packageJson, packageLock, provenance) {
  invariant(packageLock.lockfileVersion === 3, 'package-lock.json must use lockfileVersion 3')
  invariant(packageLock.packages && typeof packageLock.packages === 'object', 'package-lock packages map missing')
  const directDependencies = { ...packageJson.dependencies, ...packageJson.devDependencies }
  for (const [name, version] of Object.entries(directDependencies)) {
    invariant(EXACT_VERSION.test(version), `${name}: direct npm dependency is not exact`)
    invariant(packageLock.packages['']?.devDependencies?.[name] === version || packageLock.packages['']?.dependencies?.[name] === version, `${name}: root lock version differs from package.json`)
  }
  for (const [path, pkg] of Object.entries(packageLock.packages)) {
    if (path === '' || pkg.link) continue
    invariant(typeof pkg.version === 'string' && EXACT_VERSION.test(pkg.version), `${path}: locked npm version missing or invalid`)
    invariant(typeof pkg.resolved === 'string', `${path}: npm resolution missing`)
    const packageName = path.split('node_modules/').at(-1)
    invariant(isExpectedNpmTarball(packageName, pkg.version, pkg.resolved), `${path}: noncanonical npm registry resolution`)
    invariant(typeof pkg.integrity === 'string' && pkg.integrity.startsWith('sha512-'), `${path}: sha512 integrity missing`)
  }
  for (const direct of provenance.npm) {
    const locked = packageLock.packages[`node_modules/${direct.name}`]
    invariant(locked?.version === direct.version, `${direct.name}@${direct.version}: direct npm package missing from lock`)
    invariant(locked.integrity === direct.integrity, `${direct.name}@${direct.version}: npm integrity differs from provenance`)
  }
}

export async function verifyDependencyLocks(root = process.cwd()) {
  const crateDirectories = await readdir(join(root, 'crates'), { withFileTypes: true })
    .catch(() => [])
  const cargoMemberManifests = await Promise.all(
    crateDirectories
      .filter((entry) => entry.isDirectory())
      .map(async (entry) => ({
        path: `crates/${entry.name}/Cargo.toml`,
        source: await readFile(join(root, 'crates', entry.name, 'Cargo.toml'), 'utf8'),
      })),
  )
  const [packageJson, packageLock, cargoLock, cargoToml, config, phase1, phase2] = await Promise.all([
    readFile(join(root, 'package.json'), 'utf8').then(JSON.parse),
    readFile(join(root, 'package-lock.json'), 'utf8').then(JSON.parse),
    readFile(join(root, 'Cargo.lock'), 'utf8'),
    readFile(join(root, 'Cargo.toml'), 'utf8'),
    readFile(join(root, 'config/dependency-provenance.json'), 'utf8').then(JSON.parse),
    readFile(join(root, 'artifacts/provenance/phase1-dependencies.json'), 'utf8').then(JSON.parse),
    readFile(join(root, 'artifacts/provenance/phase2-dependencies.json'), 'utf8').then(JSON.parse),
  ])
  const provenance = mergeProvenanceReports([phase1, phase2])
  verifyManifestCoverage(packageJson, cargoToml, config, provenance, cargoMemberManifests.map(({ source }) => source))
  verifyPhase2LockIntent(packageJson, cargoToml, new Map(cargoMemberManifests.map((entry) => [entry.path, entry.source])), phase2)
  verifyCargoLock(cargoLock, provenance)
  verifyNpmLock(packageJson, packageLock, provenance)
  return { cargoPackages: parseCargoPackages(cargoLock).length, npmPackages: Object.keys(packageLock.packages).length - 1 }
}

export function isMainModule(moduleUrl, argvPath) {
  return argvPath !== undefined && resolve(fileURLToPath(moduleUrl)) === resolve(argvPath)
}

test('rejects Cargo origin and checksum drift', () => {
  const provenance = { crates: [{ name: 'serde', version: '1.0.228', checksum: 'a'.repeat(64) }] }
  const valid = `version = 4\n\n[[package]]\nname = "serde"\nversion = "1.0.228"\nsource = "${CRATES_IO_SOURCE}"\nchecksum = "${'a'.repeat(64)}"\n`
  assert.doesNotThrow(() => verifyCargoLock(valid, provenance))
  assert.throws(() => verifyCargoLock(valid.replace(CRATES_IO_SOURCE, 'git+https://example.invalid/repo'), provenance), /non-crates.io/)
  assert.throws(() => verifyCargoLock(valid.replace(/^checksum.*$/m, ''), provenance), /checksum missing/)
})

test('rejects npm origin, integrity, and floating direct-version drift', () => {
  const integrity = `sha512-${Buffer.from('integrity').toString('base64')}`
  const provenance = { npm: [{ name: 'vitest', version: '4.1.6', integrity }] }
  const packageJson = { devDependencies: { vitest: '4.1.6' } }
  const packageLock = {
    lockfileVersion: 3,
    packages: {
      '': { devDependencies: { vitest: '4.1.6' } },
      'node_modules/vitest': { version: '4.1.6', resolved: 'https://registry.npmjs.org/vitest/-/vitest-4.1.6.tgz', integrity },
    },
  }
  assert.doesNotThrow(() => verifyNpmLock(packageJson, packageLock, provenance))
  assert.throws(() => verifyNpmLock({ devDependencies: { vitest: '^4.1.6' } }, packageLock, provenance), /not exact/)
  assert.throws(() => verifyNpmLock(packageJson, { ...packageLock, packages: { ...packageLock.packages, 'node_modules/vitest': { ...packageLock.packages['node_modules/vitest'], resolved: 'https://example.invalid/vitest.tgz' } } }, provenance), /noncanonical/)
  assert.throws(() => verifyNpmLock(packageJson, { ...packageLock, packages: { ...packageLock.packages, 'node_modules/vitest': { ...packageLock.packages['node_modules/vitest'], integrity: undefined } } }, provenance), /integrity/)
  for (const resolved of [
    'https://user:password@registry.npmjs.org/vitest/-/vitest-4.1.6.tgz',
    'https://registry.npmjs.org:444/vitest/-/vitest-4.1.6.tgz',
    'https://registry.npmjs.org/other/-/vitest-4.1.6.tgz',
    'https://registry.npmjs.org/vitest/-/vitest-4.1.6.tgz?token=secret',
    'https://registry.npmjs.org/vitest/-/vitest-4.1.6.tgz#fragment',
  ]) {
    assert.throws(() => verifyNpmLock(packageJson, { ...packageLock, packages: { ...packageLock.packages, 'node_modules/vitest': { ...packageLock.packages['node_modules/vitest'], resolved } } }, provenance), /noncanonical/)
  }
})

test('runs against an isolated on-disk lock fixture', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowpdf-locks-'))
  const integrity = `sha512-${Buffer.from('fixture').toString('base64')}`
  await Promise.all([
    writeFile(join(root, 'package.json'), JSON.stringify({ devDependencies: { vitest: '4.1.6' } })),
    writeFile(join(root, 'package-lock.json'), JSON.stringify({ lockfileVersion: 3, packages: { '': { devDependencies: { vitest: '4.1.6' } }, 'node_modules/vitest': { version: '4.1.6', resolved: 'https://registry.npmjs.org/vitest/-/vitest-4.1.6.tgz', integrity } } })),
    writeFile(join(root, 'Cargo.lock'), `version = 4\n\n[[package]]\nname = "serde"\nversion = "1.0.228"\nsource = "${CRATES_IO_SOURCE}"\nchecksum = "${'a'.repeat(64)}"\n`),
    writeFile(join(root, 'Cargo.toml'), '[workspace.dependencies]\nserde = "=1.0.228"\n'),
  ])
  await import('node:fs/promises').then(({ mkdir }) => Promise.all([
    mkdir(join(root, 'artifacts/provenance'), { recursive: true }),
    mkdir(join(root, 'config'), { recursive: true }),
  ]))
  const config = { crates: [{ name: 'serde', version: '1.0.228' }], npm: [{ name: 'vitest', version: '4.1.6' }] }
  await Promise.all([
    writeFile(join(root, 'config/dependency-provenance.json'), JSON.stringify(config)),
    writeFile(join(root, 'artifacts/provenance/phase1-dependencies.json'), JSON.stringify({ status: 'success', crates: [{ name: 'serde', version: '1.0.228', kind: 'dependency', checksum: 'a'.repeat(64) }], npm: [{ name: 'vitest', version: '4.1.6', kind: 'dependency', integrity }] })),
    writeFile(join(root, 'artifacts/provenance/phase2-dependencies.json'), JSON.stringify({ status: 'success', crates: [], npm: [] })),
  ])
  assert.deepEqual(await verifyDependencyLocks(root), { cargoPackages: 1, npmPackages: 1 })
})

test('rejects omitted, extra, and version-drifted direct dependency provenance identities', () => {
  const packageJson = { devDependencies: { vitest: '4.1.6' } }
  const cargoToml = '[workspace.dependencies]\nserde = "=1.0.228"\n'
  const config = {
    crates: [{ name: 'serde', version: '1.0.228' }],
    npm: [{ name: 'vitest', version: '4.1.6' }],
  }
  const report = {
    crates: [{ name: 'serde', version: '1.0.228', kind: 'dependency' }],
    npm: [{ name: 'vitest', version: '4.1.6', kind: 'dependency' }],
  }
  assert.doesNotThrow(() => verifyManifestCoverage(packageJson, cargoToml, config, report))
  assert.throws(
    () => verifyManifestCoverage(packageJson, cargoToml, { ...config, crates: [] }, report),
    /Cargo provenance manifest identity coverage differs/,
  )
  assert.throws(
    () => verifyManifestCoverage(packageJson, cargoToml, { ...config, npm: [...config.npm, { name: 'extra', version: '1.0.0' }] }, report),
    /npm provenance manifest identity coverage differs/,
  )
  assert.throws(
    () => verifyManifestCoverage(packageJson, cargoToml, config, { ...report, npm: [{ name: 'vitest', version: '4.1.7', kind: 'dependency' }] }),
    /npm provenance report identity coverage differs/,
  )
  assert.throws(
    () => verifyManifestCoverage(
      packageJson,
      cargoToml,
      config,
      report,
      ['[dependencies]\nextra = "=1.2.3"\n'],
    ),
    /Cargo provenance manifest identity coverage differs/,
  )
})

test('rejects Phase 2 lock-intent and Cargo feature drift', () => {
  const packageJson = { dependencies: { react: '19.2.8' } }
  const cargoToml = '[workspace.dependencies]\nicu_segmenter = { version = "=2.3.0", default-features = false, features = ["compiled_data"] }\n'
  const memberManifests = new Map([
    ['crates/flow-core/Cargo.toml', '[dependencies]\nicu_segmenter.workspace = true\n'],
  ])
  const phase2 = {
    status: 'success',
    npm: [{
      name: 'react',
      version: '19.2.8',
      legitimacy: { verdict: 'OK' },
      lockIntent: { section: 'dependencies', version: '19.2.8' },
    }],
    crates: [{
      name: 'icu_segmenter',
      version: '2.3.0',
      legitimacy: { verdict: 'OK' },
      lockIntent: {
        manifest: 'crates/flow-core/Cargo.toml',
        version: '2.3.0',
        defaultFeatures: false,
        features: ['compiled_data'],
      },
    }],
  }
  assert.doesNotThrow(() => verifyPhase2LockIntent(packageJson, cargoToml, memberManifests, phase2))
  assert.throws(
    () => verifyPhase2LockIntent({ dependencies: { react: '19.2.7' } }, cargoToml, memberManifests, phase2),
    /package.json differs/,
  )
  assert.throws(
    () => verifyPhase2LockIntent(packageJson, cargoToml.replace('default-features = false, ', ''), memberManifests, phase2),
    /default Cargo features/,
  )
  assert.throws(
    () => verifyPhase2LockIntent(packageJson, cargoToml.replace('"compiled_data"', '"compiled_data", "serde"'), memberManifests, phase2),
    /feature set differs/,
  )
})

test('recognizes an encoded main-module path containing spaces', () => {
  const path = join(tmpdir(), 'flowpdf lock verifier', 'verify-dependency-locks.mjs')
  assert.equal(isMainModule(pathToFileURL(path), path), true)
})

if (!process.env.NODE_TEST_CONTEXT && isMainModule(import.meta.url, process.argv[1])) {
  const result = await verifyDependencyLocks()
  console.log(`verified Cargo packages: ${result.cargoPackages}`)
  console.log(`verified npm packages: ${result.npmPackages}`)
}
