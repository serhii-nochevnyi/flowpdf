import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { lstatSync, readFileSync } from 'node:fs'
import { delimiter, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { supportedNodeVersion } from './node-version.mjs'
import { assertManifest, tasks } from './phase9-validation-manifest.mjs'

export const root = fileURLToPath(new URL('../', import.meta.url))

export function pinnedEnvironment(env = process.env) {
  return {
    ...env,
    RUSTUP_HOME: resolve(root, 'work/toolchains/rustup'),
    CARGO_HOME: resolve(root, 'work/toolchains/cargo'),
    PATH: `${resolve(root, 'work/toolchains/cargo/bin')}${delimiter}${env.PATH ?? ''}`,
    CARGO_NET_OFFLINE: 'true',
    RUSTUP_AUTO_INSTALL: '0',
    npm_config_offline: 'true',
    PLAYWRIGHT_BROWSERS_PATH: resolve(root, 'work/playwright'),
  }
}

export function runBounded([command, ...args], { cwd = root, timeout = 15_000, env = pinnedEnvironment(),
  maxBuffer = 4 * 1024 * 1024, encoding = 'utf8' } = {}) {
  return spawnSync(command === 'node' ? process.execPath : command, args, {
    cwd, timeout, env, encoding, maxBuffer, killSignal: 'SIGKILL', stdio: ['ignore', 'pipe', 'pipe'],
  })
}

export function isProductInput(path) {
  if (path.split('/').some((part) => /^(?:\.planning|\.shipyard.*|\.agents|\.claude|\.codex|\.cursor|AGENTS\.md|CLAUDE\.md)$/i.test(part))) return false
  return /^(?:Cargo\.(?:toml|lock)|rust-toolchain\.toml|package(?:-lock)?\.json|(?:tsconfig(?:\.web)?)\.json|(?:vite|vitest)\.config\.ts|\.nvmrc|\.node-version)$/.test(path) ||
    /^(?:crates\/[^/]+\/(?:src\/|tests\/|data\/|Cargo\.toml$)|web\/(?:src\/|tests\/|persistence\/|[^/]+\.html$)|scripts\/|tests\/contracts\/|config\/|fixtures\/pdf-native\/|artifacts\/provenance\/|\.github\/workflows\/pdf-native-(?:note-tracer|core)\.yml$)/
      .test(path) && !/\.md$/i.test(path)
}

function fingerprint() {
  const listed = runBounded(['git', 'ls-files', '-z', '--cached', '--others', '--exclude-standard'])
  if (listed.status !== 0) throw new Error('PHASE9_FINGERPRINT_UNAVAILABLE')
  const hash = createHash('sha256')
  for (const path of [...new Set(listed.stdout.split('\0').filter(isProductInput))].sort()) {
    const file = resolve(root, path)
    const stat = lstatSync(file, { throwIfNoEntry: false })
    if (stat && (!stat.isFile() || stat.size > 64 * 1024 * 1024)) throw new Error('PHASE9_FINGERPRINT_UNAVAILABLE')
    hash.update(JSON.stringify([path, stat ? createHash('sha256').update(readFileSync(file)).digest('hex') : null]))
  }
  return hash.digest('hex')
}

export function safeDiagnostic(diagnostic) {
  if (Object.keys(diagnostic).sort().join(',') !== 'elapsedMilliseconds,exitCode,id,status' ||
      (typeof diagnostic.id !== 'string' || !/^09-\d{2}-\d{2}$/.test(diagnostic.id)) ||
      !['pass', 'fail', 'unavailable'].includes(diagnostic.status) ||
      !Number.isSafeInteger(diagnostic.exitCode) || diagnostic.exitCode < 0 || diagnostic.exitCode > 255 ||
      !Number.isSafeInteger(diagnostic.elapsedMilliseconds) || diagnostic.elapsedMilliseconds < 0) {
    throw new Error('PHASE9_UNSAFE_DIAGNOSTIC')
  }
  return diagnostic
}

export function runTasks(lane, run = runBounded) {
  if (!['local', 'smoke', 'browser', 'release'].includes(lane)) throw new Error('PHASE9_INVALID_LANE')
  assertManifest()
  const diagnostics = []
  for (const task of tasks.filter((task) => task.lane === lane || (lane === 'local' && task.lane === 'smoke'))) {
    const start = Date.now()
    const result = run(task.argv, { timeout: task.timeout })
    const exitCode = result.status ?? 1
    const status = exitCode === 0 ? 'pass' :
      (result.error?.code === 'ENOENT' || (lane === 'release' && exitCode === 2) ||
        (task.id === '09-03-02' && /^NATIVE_NOTE_TRACER_[A-Z_]+_UNAVAILABLE\s*$/.test(result.stderr ?? '')))
        ? 'unavailable' : 'fail'
    diagnostics.push(safeDiagnostic({ id: task.id, status, exitCode, elapsedMilliseconds: Math.max(0, Date.now() - start) }))
    if (status !== 'pass') return { lane, status, exitCode, tasks: diagnostics }
  }
  return { lane, status: 'pass', exitCode: 0, tasks: diagnostics }
}

function preflight(lane) {
  if (lane === 'release') return null
  if (process.versions.node !== supportedNodeVersion) return 'node-24.10.0'
  if (lane === 'smoke') return null
  const npm = runBounded(['npm', '--version'])
  if (npm.status !== 0 || npm.stdout.trim() !== '11.9.0') return 'npm-11.9.0'
  if (lane === 'local') {
    const rust = runBounded(['rustc', '--version'])
    if (rust.status !== 0 || !/^rustc 1\.97\.1 /.test(rust.stdout)) return 'rust-1.97.1'
    const lock = JSON.parse(readFileSync(resolve(root, 'package-lock.json'), 'utf8')).packages
    const project = JSON.parse(readFileSync(resolve(root, 'package.json'), 'utf8'))
    for (const name of Object.keys({ ...project.dependencies, ...project.devDependencies })) {
      try {
        if (JSON.parse(readFileSync(resolve(root, 'node_modules', name, 'package.json'), 'utf8')).version !==
            lock[`node_modules/${name}`]?.version) return 'locked-node-dependencies'
      } catch {
        return 'locked-node-dependencies'
      }
    }
  }
  return null
}

const flags = new Map([['--smoke', 'smoke'], ['--browser', 'browser'], ['--strict-external', 'release']])

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2)
  const lane = args.length === 0 ? 'local' : args.length === 1 ? flags.get(args[0]) : null
  let result = { status: 'fail', exitCode: 1, tasks: [] }
  try {
    if (!lane) throw new Error('PHASE9_INVALID_LANE')
    const missing = preflight(lane)
    if (missing) {
      result = { lane, status: 'unavailable', exitCode: 2, tasks: [], missing }
    } else {
      const before = fingerprint()
      let evidence
      result = runTasks(lane, (argv, options) => {
        const child = runBounded(argv, options)
        if (lane === 'release') {
          try {
            evidence = JSON.parse(child.stdout)
          } catch {
            evidence = undefined
          }
          const expected = new Map([[0, 'pass'], [1, 'fail'], [2, 'unavailable']]).get(child.status)
          if (!expected || evidence?.status !== expected ||
              (expected === 'pass' && evidence.fixtures?.length !== 4)) {
            evidence = undefined
            return child.error?.code === 'ENOENT' ? child : { status: 1 }
          }
        }
        return child
      })
      if (fingerprint() !== before) result = { ...result, status: 'fail', exitCode: 1, reason: 'product-input-changed' }
      else if (evidence) result.evidence = evidence
    }
  } catch {
    result = { ...result, lane, status: 'fail', exitCode: 1, reason: 'gate-preflight-failed' }
  }
  console.log(JSON.stringify(result))
  process.exitCode = result.exitCode
}
