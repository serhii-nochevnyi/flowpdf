import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { buildArgv, browserArgv, bundleArgv, builtBrowserArgv, requirePassedCi, runTracer } from '../../scripts/check-native-note-tracer.mjs'

const workflowText = readFileSync(new URL('../../.github/workflows/pdf-native-note-tracer.yml', import.meta.url), 'utf8')
const sha = 'a'.repeat(40)
const repo = 'serhii-nochevnyi/flowpdf'
const workflow = 'Native PDF note tracer'
const name = 'native-pdf-note-tracer'
const url = `https://api.github.com/repos/${repo}/check-runs/73`

function fixture() {
  const run = {
    id: 7, run_number: 2, run_attempt: 3, head_sha: sha, event: 'push',
    name: workflow, path: '.github/workflows/pdf-native-note-tracer.yml',
    repository: { full_name: repo }, status: 'completed', conclusion: 'success',
  }
  const job = {
    id: 73, name, head_sha: sha, workflow_name: workflow, run_id: run.id, check_run_url: url,
    labels: ['ubuntu-24.04'], status: 'completed', conclusion: 'success',
    steps: [
      'Verify exact checkout', 'Build generated WASM and web output',
      'Run bilingual Chromium note tracer', 'Bundle the production editor',
      'Run built-editor Chromium smoke', 'Verify source checkout stayed unchanged',
    ].map((name, index) => ({ name, number: index + 2, status: 'completed', conclusion: 'success' })),
  }
  const check = {
    name, head_sha: sha, url, status: 'completed', conclusion: 'success',
    app: { slug: 'github-actions', owner: { login: 'github' } },
  }
  const state = { runs: [run], jobs: [job], checks: [check], origin: `git@github.com:${repo}.git`, auth: true, workflowText }
  function command(argv) {
    if (argv[1] === 'rev-parse') {
      assert.deepEqual(argv, ['git', 'rev-parse', '--verify', '--end-of-options', 'HEAD^{commit}'])
      return sha
    }
    if (argv[1] === 'show') return state.workflowText
    if (argv[1] === 'remote') return state.origin
    if (argv[1] === 'auth') {
      if (!state.auth) throw new Error('auth unavailable')
      return ''
    }
    assert.deepEqual(argv.slice(0, 8), ['gh', 'api', '--hostname', 'github.com', '--method', 'GET', '-H',
      'Accept: application/vnd.github+json'])
    const endpoint = argv.at(-1)
    if (endpoint === `repos/${repo}/actions/workflows/pdf-native-note-tracer.yml/runs?event=push&head_sha=${sha}&per_page=100`) {
      return JSON.stringify({ workflow_runs: state.runs, total_count: state.runs.length })
    }
    if (endpoint === `repos/${repo}/actions/runs/7/attempts/3/jobs?per_page=100`) {
      return JSON.stringify({ jobs: state.jobs, total_count: state.jobs.length })
    }
    assert.equal(endpoint, `repos/${repo}/commits/${sha}/check-runs?check_name=${name}&filter=all&per_page=100`)
    return JSON.stringify({ check_runs: state.checks, total_count: state.checks.length })
  }
  return { run, job, check, state, command }
}

test('requires the configured workflow, current attempt, and exact Actions check on the inherited SHA', () => {
  assert.deepEqual(requirePassedCi('HEAD', fixture().command), {
    repository: repo, sha, run: 7, attempt: 3, job: 73,
  })
})

const refusals = [
  ['wrong origin', ({ state }) => { state.origin = 'https://github.com/another/flowpdf' }],
  ['unauthenticated', ({ state }) => { state.auth = false }],
  ['missing run', ({ state }) => { state.runs = [] }],
  ['wrong run SHA', ({ run }) => { run.head_sha = 'b'.repeat(40) }],
  ['wrong run repository', ({ run }) => { run.repository.full_name = 'another/flowpdf' }],
  ['modified workflow bytes', ({ state }) => { state.workflowText += ' ' }],
  ['pull request run', ({ run }) => { run.event = 'pull_request' }],
  ['missing mandatory step', ({ job }) => { job.steps.pop() }],
  ['duplicate mandatory step', ({ job }) => { job.steps.push(job.steps[0]) }],
  ['wrong workflow',  ({ run }) => { run.path = '.github/workflows/impostor.yml' }],
  ['outdated success', ({ run, state }) => { state.runs.push({ ...run, run_number: 3, status: 'queued' }) }],
  ['wrong job', ({ job }) => { job.name = 'impostor' }],
  ['wrong attempt', ({ job }) => { job.run_attempt = 2 }],
  ['wrong runner', ({ job }) => { job.labels = ['self-hosted'] }],
  ['skipped build', ({ job }) => { job.steps[0].conclusion = 'skipped' }],
  ['missing check', ({ state }) => { state.checks = [] }],
  ['wrong check SHA', ({ check }) => { check.head_sha = 'b'.repeat(40) }],
  ['wrong check identity', ({ check }) => { check.url = `${url}0` }],
  ['wrong app', ({ check }) => { check.app.slug = 'other-actions' }],
  ...['neutral', 'skipped', 'cancelled', 'failure'].map((status) =>
    [`check conclusion ${status}`, ({ check }) => { check.conclusion = status }]),
]

for (const [description, mutate] of refusals) {
  test(`rejects ${description}`, () => {
    const mock = fixture()
    mutate(mock)
    assert.throws(() => requirePassedCi('HEAD', mock.command), /NATIVE_NOTE_TRACER_/)
  })
}

function runner({ fail, npm = '11.9.0' } = {}) {
  const calls = []
  return {
    calls,
    command(argv, options) {
      calls.push(argv)
      if (argv[0] === 'git') return sha
      assert.equal(options.env.PLAYWRIGHT_BROWSERS_PATH, './work/playwright')
      assert.equal(options.env.CARGO_NET_OFFLINE, 'true')
      if (argv.join(' ') === fail?.join(' ')) throw new Error('private subprocess diagnostic')
      if (argv[0] === 'npm' && argv[1] === '--version') return npm
      if (argv[0] === 'rustc') return 'rustc 1.97.1 (fixture)'
      if (argv[0] === 'rustup') return 'wasm32-unknown-unknown'
      return ''
    },
  }
}

test('runs source and built Chromium checks with a pinned cache', () => {
  const mock = runner()
  assert.deepEqual(runTracer(mock.command, {}, '24.10.0'), { sha })
  assert.deepEqual(mock.calls.slice(-5, -1), [buildArgv, browserArgv, bundleArgv, builtBrowserArgv])
})

test('fails closed on missing tools, checkout mismatch, and build or browser failure', () => {
  const oldNode = runner()
  assert.throws(() => runTracer(oldNode.command, {}, '24.9.0'), /NODE_UNAVAILABLE/)
  assert.deepEqual(oldNode.calls, [])
  const oldNpm = runner({ npm: '11.8.0' })
  assert.throws(() => runTracer(oldNpm.command, {}, '24.10.0'), /NPM_UNAVAILABLE/)
  const wrongHead = runner()
  assert.throws(() => runTracer(wrongHead.command, { GITHUB_ACTIONS: 'true' }, '24.10.0'), /CHECKOUT_MISMATCH/)
  const build = runner({ fail: buildArgv })
  assert.throws(() => runTracer(build.command, {}, '24.10.0'), { message: 'NATIVE_NOTE_TRACER_BUILD_FAILED' })
  assert.deepEqual(build.calls.at(-1), buildArgv)
  const browser = runner({ fail: browserArgv })
  assert.throws(() => runTracer(browser.command, {}, '24.10.0'), { message: 'NATIVE_NOTE_TRACER_BROWSER_FAILED' })
  assert.deepEqual(browser.calls.at(-1), browserArgv)
})

test('fails closed when the production bundle or its Chromium smoke fails', () => {
  for (const [argv, reason] of [
    [bundleArgv, 'NATIVE_NOTE_TRACER_BUNDLE_FAILED'],
    [builtBrowserArgv, 'NATIVE_NOTE_TRACER_BUILT_BROWSER_FAILED'],
  ]) {
    const mock = runner({ fail: argv })
    assert.throws(() => runTracer(mock.command, {}, '24.10.0'), { message: reason })
    assert.deepEqual(mock.calls.at(-1), argv)
  }
})
