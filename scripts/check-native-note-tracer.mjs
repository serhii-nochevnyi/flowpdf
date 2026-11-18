import { execFileSync } from 'node:child_process'
import { delimiter, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const repository = 'serhii-nochevnyi/flowpdf'
const workflowFile = 'pdf-native-note-tracer.yml'
const workflowPath = `.github/workflows/${workflowFile}`
const workflowName = 'Native PDF note tracer'
const jobName = 'native-pdf-note-tracer'
const runnerLabel = 'flowpdf-phase9'
const buildStep = 'Build and trace native notes'
const shaPattern = /^[a-f0-9]{40}$/
const apiTimeout = 20_000

function requireEvidence(condition, code) {
  if (!condition) throw new Error(code)
}

function execute(argv, { timeout = apiTimeout, env = process.env } = {}) {
  const [command, ...args] = argv
  // Keep subprocess output out of gate diagnostics, including gh auth errors.
  return execFileSync(command, args, {
    cwd: root, env, encoding: 'utf8', timeout, maxBuffer: 8 * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
  }).trim()
}

function call(command, argv, code, options) {
  try {
    return command(argv, options)
  } catch {
    throw new Error(code)
  }
}

function resolveRef(command, ref) {
  requireEvidence(typeof ref === 'string' && ref.length > 0 && ref.length <= 256,
    'NATIVE_NOTE_TRACER_INVALID_REF')
  const sha = call(command, ['git', 'rev-parse', '--verify', '--end-of-options', `${ref}^{commit}`],
    'NATIVE_NOTE_TRACER_REF_UNAVAILABLE')
  requireEvidence(shaPattern.test(sha), 'NATIVE_NOTE_TRACER_INVALID_SHA')
  return sha
}

export function requirePassedCi(ref, command = execute) {
  const sha = resolveRef(command, ref)
  const origin = call(command, ['git', 'remote', 'get-url', 'origin'], 'NATIVE_NOTE_TRACER_ORIGIN_UNAVAILABLE')
  requireEvidence(
    [`https://github.com/${repository}`, `git@github.com:${repository}`, `ssh://git@github.com/${repository}`]
      .includes(origin.replace(/\.git$/, '').toLowerCase()),
    'NATIVE_NOTE_TRACER_WRONG_REPOSITORY',
  )
  call(command, ['gh', 'auth', 'status', '--hostname', 'github.com'],
    'NATIVE_NOTE_TRACER_CI_UNAVAILABLE')
  function get(endpoint) {
    try {
      return JSON.parse(command(['gh', 'api', '--hostname', 'github.com', '--method', 'GET',
        '-H', 'Accept: application/vnd.github+json', `repos/${repository}/${endpoint}`]))
    } catch {
      throw new Error('NATIVE_NOTE_TRACER_CI_UNAVAILABLE')
    }
  }

  // Workflow-specific lookup includes newer queued runs that have no check yet.
  const { workflow_runs: runs, total_count: runCount } = get(`actions/workflows/${workflowFile}/runs?head_sha=${sha}&per_page=100`)
  requireEvidence(Array.isArray(runs) && runs.length > 0 && runs.length < 100 && runCount === runs.length &&
    runs.every((run) => Number.isSafeInteger(run.run_number)), 'NATIVE_NOTE_TRACER_RUN_UNAVAILABLE')
  const [run] = [...runs].sort((a, b) => b.run_number - a.run_number)
  requireEvidence(run?.head_sha === sha && run.repository?.full_name?.toLowerCase() === repository &&
    run.name === workflowName && run.path === workflowPath &&
    ['push', 'pull_request'].includes(run.event) &&
    Number.isSafeInteger(run.id) && run.id > 0 &&
    Number.isSafeInteger(run.run_attempt) && run.run_attempt > 0 &&
    run.status === 'completed' && run.conclusion === 'success', 'NATIVE_NOTE_TRACER_RUN_NOT_PASSED')

  const { jobs, total_count: jobCount } = get(`actions/runs/${run.id}/attempts/${run.run_attempt}/jobs?per_page=100`)
  const matchingJobs = Array.isArray(jobs) && jobCount === jobs.length && jobs.length < 100
    ? jobs.filter((job) => job.name === jobName) : []
  const [job] = matchingJobs
  requireEvidence(matchingJobs.length === 1 && job.run_id === run.id &&
    (job.run_attempt === undefined || job.run_attempt === run.run_attempt) && job.head_sha === sha &&
    Number.isSafeInteger(job.id) && job.id > 0 &&
    typeof job.check_run_url === 'string' &&
    /^https:\/\/api\.github\.com\/repos\/serhii-nochevnyi\/flowpdf\/check-runs\/\d+$/.test(job.check_run_url) &&
    job.workflow_name === workflowName && job.labels?.includes('self-hosted') &&
    job.labels?.includes(runnerLabel) && job.status === 'completed' && job.conclusion === 'success' &&
    job.steps?.some((step) => step.name === buildStep && step.status === 'completed' && step.conclusion === 'success'),
  'NATIVE_NOTE_TRACER_JOB_NOT_PASSED')

  const { check_runs: checks, total_count: count } = get(
    `commits/${sha}/check-runs?check_name=${jobName}&filter=all&per_page=100`,
  )
  requireEvidence(Array.isArray(checks) && count === checks.length && checks.length < 100,
    'NATIVE_NOTE_TRACER_CHECK_UNAVAILABLE')
  const matchingChecks = checks.filter((check) => check.url === job.check_run_url &&
    check.name === jobName && check.head_sha === sha && check.app?.slug === 'github-actions' &&
    check.app?.owner?.login === 'github')
  requireEvidence(matchingChecks.length === 1 && matchingChecks[0].status === 'completed' &&
    matchingChecks[0].conclusion === 'success', 'NATIVE_NOTE_TRACER_CHECK_NOT_PASSED')
  return { repository, sha, run: run.id, attempt: run.run_attempt, job: job.id }
}

export const buildArgv = ['npm', 'run', 'build:web']
export const browserArgv = ['npx', '--no-install', 'vitest', 'run', '--project', 'browser',
  'web/tests/pdf-native-edit.browser.test.ts']

export function runTracer(command = execute, env = process.env, nodeVersion = process.versions.node) {
  requireEvidence(nodeVersion === '24.10.0', 'NATIVE_NOTE_TRACER_NODE_UNAVAILABLE')
  const pinnedEnv = {
    ...env,
    RUSTUP_HOME: resolve(root, 'work/toolchains/rustup'),
    CARGO_HOME: resolve(root, 'work/toolchains/cargo'),
    PATH: `${resolve(root, 'work/toolchains/cargo/bin')}${delimiter}${env.PATH ?? ''}`,
    CARGO_NET_OFFLINE: 'true',
    npm_config_offline: 'true',
    PLAYWRIGHT_BROWSERS_PATH: './work/playwright',
  }
  const options = { env: pinnedEnv }
  const npm = call(command, ['npm', '--version'], 'NATIVE_NOTE_TRACER_NPM_UNAVAILABLE', options)
  requireEvidence(npm === '11.9.0', 'NATIVE_NOTE_TRACER_NPM_UNAVAILABLE')
  const rust = call(command, ['rustc', '--version'], 'NATIVE_NOTE_TRACER_RUST_UNAVAILABLE', options)
  requireEvidence(/^rustc 1\.97\.1 /.test(rust), 'NATIVE_NOTE_TRACER_RUST_UNAVAILABLE')
  const targets = call(command, ['rustup', 'target', 'list', '--installed'], 'NATIVE_NOTE_TRACER_WASM_UNAVAILABLE', options)
  requireEvidence(targets.split(/\s+/).includes('wasm32-unknown-unknown'), 'NATIVE_NOTE_TRACER_WASM_UNAVAILABLE')
  call(command, ['node', '--input-type=module', '-e', [
    "import { readFileSync } from 'node:fs'",
    "const read = (file) => JSON.parse(readFileSync(file, 'utf8'))",
    "const manifest = read('package.json')",
    "const lock = read('package-lock.json').packages",
    "for (const name of Object.keys({ ...manifest.dependencies, ...manifest.devDependencies })) {",
    "  const key = 'node_modules/' + name",
    "  if (!lock[key] || read(key + '/package.json').version !== lock[key].version) process.exit(1)",
    "}",
  ].join('\n')], 'NATIVE_NOTE_TRACER_DEPENDENCIES_UNAVAILABLE', options)
  call(command, ['node', 'scripts/verify-wasm-bindgen-tool.mjs'], 'NATIVE_NOTE_TRACER_BINDGEN_UNAVAILABLE', options)
  call(command, ['node', '--input-type=module', '-e', [
    "import { chromium } from 'playwright'",
    "const browser = await chromium.launch({ headless: true, args: ['--single-process'] })",
    'await browser.close()',
  ].join('\n')], 'NATIVE_NOTE_TRACER_CHROMIUM_UNAVAILABLE', options)

  const sha = resolveRef(command, 'HEAD')
  if (env.GITHUB_ACTIONS === 'true') {
    requireEvidence(env.GITHUB_REPOSITORY?.toLowerCase() === repository &&
      env.FLOWPDF_TRACER_HEAD === sha && env.GITHUB_JOB === 'native_pdf_note_tracer' &&
      /^\d+$/.test(env.GITHUB_RUN_ID ?? '') && /^\d+$/.test(env.GITHUB_RUN_ATTEMPT ?? ''),
    'NATIVE_NOTE_TRACER_CHECKOUT_MISMATCH')
    call(command, ['git', 'diff', '--quiet', 'HEAD', '--'], 'NATIVE_NOTE_TRACER_CHECKOUT_MISMATCH')
  }
  call(command, buildArgv, 'NATIVE_NOTE_TRACER_BUILD_FAILED', { ...options, timeout: 20 * 60_000 })
  call(command, browserArgv, 'NATIVE_NOTE_TRACER_BROWSER_FAILED', { ...options, timeout: 5 * 60_000 })
  requireEvidence(resolveRef(command, 'HEAD') === sha, 'NATIVE_NOTE_TRACER_CHECKOUT_MISMATCH')
  if (env.GITHUB_ACTIONS === 'true') {
    call(command, ['git', 'diff', '--quiet', 'HEAD', '--'], 'NATIVE_NOTE_TRACER_CHECKOUT_MISMATCH')
  }
  return { sha }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2)
  try {
    if (args.length === 1 && args[0] === '--run') {
      const { sha } = runTracer()
      const identity = process.env.GITHUB_ACTIONS === 'true' ? {
        repository,
        run: process.env.GITHUB_RUN_ID,
        attempt: process.env.GITHUB_RUN_ATTEMPT,
        job: jobName,
      } : {}
      console.log(JSON.stringify({ ...identity, sha, build: 'passed', browser: 'passed' }))
    } else if (args.length === 3 && args[0] === '--require-passed-ci' && args[1] === '--ref') {
      console.log(JSON.stringify(requirePassedCi(args[2])))
    } else {
      throw new Error('NATIVE_NOTE_TRACER_INVALID_ARGUMENTS')
    }
  } catch (error) {
    const code = error instanceof Error && /^NATIVE_NOTE_TRACER_[A-Z_]+$/.test(error.message)
      ? error.message : 'NATIVE_NOTE_TRACER_UNAVAILABLE'
    console.error(code)
    process.exitCode = 1
  }
}
