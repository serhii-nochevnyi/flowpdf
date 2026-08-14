import assert from 'node:assert/strict'
import { mkdtemp, readFile, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
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
    const resolved = new URL(pkg.resolved)
    invariant(resolved.protocol === 'https:' && resolved.hostname === 'registry.npmjs.org', `${path}: non-registry npm resolution`)
    invariant(typeof pkg.integrity === 'string' && pkg.integrity.startsWith('sha512-'), `${path}: sha512 integrity missing`)
  }
  for (const direct of provenance.npm) {
    const locked = packageLock.packages[`node_modules/${direct.name}`]
    invariant(locked?.version === direct.version, `${direct.name}@${direct.version}: direct npm package missing from lock`)
    invariant(locked.integrity === direct.integrity, `${direct.name}@${direct.version}: npm integrity differs from provenance`)
  }
}

export async function verifyDependencyLocks(root = process.cwd()) {
  const [packageJson, packageLock, cargoLock, provenance] = await Promise.all([
    readFile(join(root, 'package.json'), 'utf8').then(JSON.parse),
    readFile(join(root, 'package-lock.json'), 'utf8').then(JSON.parse),
    readFile(join(root, 'Cargo.lock'), 'utf8'),
    readFile(join(root, 'artifacts/provenance/phase1-dependencies.json'), 'utf8').then(JSON.parse),
  ])
  invariant(provenance.status === 'success', 'dependency provenance report is not successful')
  verifyCargoLock(cargoLock, provenance)
  verifyNpmLock(packageJson, packageLock, provenance)
  return { cargoPackages: parseCargoPackages(cargoLock).length, npmPackages: Object.keys(packageLock.packages).length - 1 }
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
  assert.throws(() => verifyNpmLock(packageJson, { ...packageLock, packages: { ...packageLock.packages, 'node_modules/vitest': { ...packageLock.packages['node_modules/vitest'], resolved: 'https://example.invalid/vitest.tgz' } } }, provenance), /non-registry/)
  assert.throws(() => verifyNpmLock(packageJson, { ...packageLock, packages: { ...packageLock.packages, 'node_modules/vitest': { ...packageLock.packages['node_modules/vitest'], integrity: undefined } } }, provenance), /integrity/)
})

test('runs against an isolated on-disk lock fixture', async () => {
  const root = await mkdtemp(join(tmpdir(), 'flowpdf-locks-'))
  const integrity = `sha512-${Buffer.from('fixture').toString('base64')}`
  await Promise.all([
    writeFile(join(root, 'package.json'), JSON.stringify({ devDependencies: { vitest: '4.1.6' } })),
    writeFile(join(root, 'package-lock.json'), JSON.stringify({ lockfileVersion: 3, packages: { '': { devDependencies: { vitest: '4.1.6' } }, 'node_modules/vitest': { version: '4.1.6', resolved: 'https://registry.npmjs.org/vitest/-/vitest-4.1.6.tgz', integrity } } })),
    writeFile(join(root, 'Cargo.lock'), `version = 4\n\n[[package]]\nname = "serde"\nversion = "1.0.228"\nsource = "${CRATES_IO_SOURCE}"\nchecksum = "${'a'.repeat(64)}"\n`),
  ])
  await import('node:fs/promises').then(({ mkdir }) => mkdir(join(root, 'artifacts/provenance'), { recursive: true }))
  await writeFile(join(root, 'artifacts/provenance/phase1-dependencies.json'), JSON.stringify({ status: 'success', crates: [{ name: 'serde', version: '1.0.228', checksum: 'a'.repeat(64) }], npm: [{ name: 'vitest', version: '4.1.6', integrity }] }))
  assert.deepEqual(await verifyDependencyLocks(root), { cargoPackages: 1, npmPackages: 1 })
})

if (!process.env.NODE_TEST_CONTEXT && process.argv[1] && new URL(import.meta.url).pathname === process.argv[1]) {
  const result = await verifyDependencyLocks()
  console.log(`verified Cargo packages: ${result.cargoPackages}`)
  console.log(`verified npm packages: ${result.npmPackages}`)
}
