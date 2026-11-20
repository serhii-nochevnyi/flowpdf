import assert from 'node:assert/strict'
import { execFileSync, spawnSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import { buildArgv, browserArgv, bundleArgv, builtBrowserArgv, requirePassedCi, runTracer } from '../../scripts/check-native-note-tracer.mjs'

const sha = 'a'.repeat(40)
const repo = 'serhii-nochevnyi/flowpdf'
const workflow = 'Native PDF note tracer'
const name = 'native-pdf-note-tracer'
const url = `https://api.github.com/repos/${repo}/check-runs/73`
const root = new URL('../../', import.meta.url)
const browserSuites = ['edit', 'text', 'objects', 'pages', 'forms', 'redaction']
  .map((suite) => `web/tests/pdf-native-${suite}.browser.test.ts`)
const baselineRevision = '5683541fe572a4d88d36df15db37ff6cde85f828'
const verifiedRasterRevision = 'b3340c7aa5919715320d65548463fdff68cb2e43'
const olderPageRevision = '793ff4c687a71210205258e58003400c7738affa'
const recoveryNoteRevision = '6f4b7ea593f2efd3ac3237cf8c3f1660ad0d4263'
const accessibleTextRevision = 'e6fb68578bc5d70358d0d4e9f20f78c61015ad6e'
const previousAccessibleTextRevision = 'f5fac52c09641cdd80bdd7883d9a4ff4c90276a7'
const readbackRevision = '6acf576f5d5e7957af6cfa9e2492e6c3a3edbcee'
const objectsSuite = 'web/tests/pdf-native-objects.browser.test.ts'
const pagesSuite = 'web/tests/pdf-native-pages.browser.test.ts'
const redactionSuite = 'web/tests/pdf-native-redaction.browser.test.ts'

function fixture({ expanded = true, updatedPackage = true, historicalFiles = new Map(), revision = sha } = {}) {
  const run = {
    id: 7, run_number: 2, run_attempt: 3, head_sha: revision, event: 'push',
    name: workflow, path: '.github/workflows/pdf-native-note-tracer.yml',
    repository: { full_name: repo }, status: 'completed', conclusion: 'success',
  }
  const job = {
    id: 73, name, head_sha: revision, workflow_name: workflow, run_id: run.id, check_run_url: url,
    labels: ['ubuntu-24.04'], status: 'completed', conclusion: 'success',
    steps: [
      'Verify exact checkout', 'Build generated WASM and web output',
      'Run bilingual Chromium note tracer',
      ...(expanded ? [
        'Run bilingual Chromium text replacement tracer',
        'Run bilingual Chromium object editing tracer',
        'Run bilingual Chromium page management tracer',
        'Run bilingual Chromium field editing tracer',
        'Run bilingual Chromium redaction tracer',
      ] : []),
      'Bundle the production editor',
      'Run built-editor Chromium smoke', 'Verify source checkout stayed unchanged',
    ].map((name, index) => ({ name, number: index + 2, status: 'completed', conclusion: 'success' })),
  }
  const check = {
    name, head_sha: revision, url, status: 'completed', conclusion: 'success',
    app: { slug: 'github-actions', owner: { login: 'github' } },
  }
  const state = { runs: [run], jobs: [job], checks: [check], origin: `git@github.com:${repo}.git`, auth: true, changedPath: null }
  function command(argv) {
    if (argv[1] === 'rev-parse') {
      assert.deepEqual(argv, ['git', 'rev-parse', '--verify', '--end-of-options', 'HEAD^{commit}'])
      return revision
    }
    if (argv[1] === 'show') {
      assert.ok(argv[2].startsWith(`${revision}:`))
      const path = argv[2].slice(revision.length + 1)
      if (historicalFiles.has(path)) {
        const contents = historicalFiles.get(path)
        return path === state.changedPath ? contents + ' ' : contents
      }
      let bytes = readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8')
      if (!expanded && path === run.path) {
        bytes = bytes.replace(
          /^      - name: Run bilingual Chromium (?:text replacement|object editing|page management|field editing|redaction) tracer\n        run: [^\n]+\n/gm,
          '',
        )
      }
      if (!updatedPackage && path === 'package.json') {
        bytes = bytes.replace(/^    "check:phase9(?:[:a-z]+)?": .*\n/gm, '')
      }
      return path === state.changedPath ? bytes + ' ' : bytes
    }
    if (argv[1] === 'remote') return state.origin
    if (argv[1] === 'auth') {
      if (!state.auth) throw new Error('auth unavailable')
      return ''
    }
    assert.deepEqual(argv.slice(0, 8), ['gh', 'api', '--hostname', 'github.com', '--method', 'GET', '-H',
      'Accept: application/vnd.github+json'])
    const endpoint = argv.at(-1)
    if (endpoint === `repos/${repo}/actions/workflows/pdf-native-note-tracer.yml/runs?event=push&head_sha=${revision}&per_page=100`) {
      return JSON.stringify({ workflow_runs: state.runs, total_count: state.runs.length })
    }
    if (endpoint === `repos/${repo}/actions/runs/7/attempts/3/jobs?per_page=100`) {
      return JSON.stringify({ jobs: state.jobs, total_count: state.jobs.length })
    }
    assert.equal(endpoint, `repos/${repo}/commits/${revision}/check-runs?check_name=${name}&filter=all&per_page=100`)
    return JSON.stringify({ check_runs: state.checks, total_count: state.checks.length })
  }
  return { run, job, check, state, command }
}

function harnessFilesFrom(t, revision, paths) {
  const options = { cwd: root, encoding: 'utf8' }
  const files = new Map()
  for (const path of paths) {
    const blob = spawnSync('git', ['show', `${revision}:${path}`], options)
    assert.ifError(blob.error)
    if (blob.status !== 0 && execFileSync('git', ['rev-parse', '--is-shallow-repository'], options).trim() === 'true') {
      t.skip(`Harness bytes for ${revision} are unavailable in this shallow checkout`)
      return null
    }
    assert.equal(blob.status, 0, blob.stderr)
    files.set(path, blob.stdout)
  }
  return files
}

test('requires the configured workflow, current attempt, and exact Actions check on the inherited SHA', () => {
  assert.deepEqual(requirePassedCi('HEAD', fixture().command), {
    repository: repo, sha, run: 7, attempt: 3, job: 73,
  })
})

test('requires the expanded workflow and package for the current browser harness', () => {
  for (const options of [{ expanded: false, updatedPackage: false }, { updatedPackage: false }, { expanded: false }]) {
    assert.throws(() => requirePassedCi('HEAD', fixture(options).command), /HARNESS_NOT_APPROVED/)
  }
})

for (const [profile, revision, expanded] of [
  ['historical note-only', '1e6cbafbcdbe1cd8d74fdcaca71dbcf24665a358', false],
  ['historical expanded', '09ae65788a803835395e07beaa9240ab78239426', true],
  ['previous reviewed page and redaction', '9a14e505d07f341beaa0af023da2c2ae4b095758', true],
  ['previous reviewed page and stroke', olderPageRevision, true],
  ['previous reviewed preview and redaction', '956ae26938bb952bba2339a18db3d8b46db7b88e', true],
  ['previous reviewed verified-raster redaction (b3340c7)', verifiedRasterRevision, true],
  ['reviewed baseline (5683541)', baselineRevision, true],
  ['reviewed recovery note', recoveryNoteRevision, true],
  ['reviewed accessible page text', accessibleTextRevision, true],
  ['reviewed worker quarantine and accessible text', 'fb250eb5e22f5c2220eb79c94a08e08a006faaed', true],
  ['previous reviewed worker quarantine and accessible text', previousAccessibleTextRevision, true],
  ['reviewed readback', readbackRevision, true],
]) {
  test(`accepts the exact ${profile} harness`, (t) => {
    const paths = ['.github/workflows/pdf-native-note-tracer.yml', 'package.json',
      ...browserSuites.slice(0, expanded ? browserSuites.length : 1)]
    const historicalFiles = harnessFilesFrom(t, revision, paths)
    if (!historicalFiles) return
    const { command, state } = fixture({ expanded, historicalFiles, revision })
    assert.deepEqual(requirePassedCi('HEAD', command), { repository: repo, sha: revision, run: 7, attempt: 3, job: 73 })
    for (const path of expanded ? [browserSuites[0], objectsSuite, pagesSuite, redactionSuite] : [browserSuites[0]]) {
      state.changedPath = path
      assert.throws(() => requirePassedCi('HEAD', command), { message: 'NATIVE_NOTE_TRACER_HARNESS_NOT_APPROVED' })
    }
  })
}

for (const [profile, revision] of [['5683541', baselineRevision], ['b3340c7', verifiedRasterRevision]]) {
  test(`refuses the ${profile} redaction suite with the older reviewed page suite`, (t) => {
    const historicalFiles = harnessFilesFrom(t, revision, ['.github/workflows/pdf-native-note-tracer.yml',
      'package.json', ...browserSuites])
    const olderPage = harnessFilesFrom(t, olderPageRevision, [pagesSuite])
    if (!historicalFiles || !olderPage) return
    historicalFiles.set(pagesSuite, olderPage.get(pagesSuite))
    const { command } = fixture({ historicalFiles, revision })
    assert.throws(() => requirePassedCi('HEAD', command), { message: 'NATIVE_NOTE_TRACER_HARNESS_NOT_APPROVED' })
  })
}

for (const [description, suite, revision] of [
  ['the older page suite', pagesSuite, olderPageRevision],
  ['the verified raster suite', redactionSuite, verifiedRasterRevision],
  ['the previous navigation suite', browserSuites[0], previousAccessibleTextRevision],
  ['the previous redaction suite', redactionSuite, previousAccessibleTextRevision],
]) {
  test(`refuses the current accessible page text profile with ${description}`, (t) => {
    const historicalFiles = harnessFilesFrom(t, revision, [suite])
    if (!historicalFiles) return
    const { command } = fixture({ historicalFiles })
    assert.throws(() => requirePassedCi('HEAD', command), { message: 'NATIVE_NOTE_TRACER_HARNESS_NOT_APPROVED' })
  })
}

test('refuses the recovery note suite with a different reviewed redaction suite', (t) => {
  const recovery = harnessFilesFrom(t, recoveryNoteRevision, [browserSuites[0]])
  const redaction = harnessFilesFrom(t, verifiedRasterRevision, [redactionSuite])
  if (!recovery || !redaction) return
  const { command } = fixture({ historicalFiles: new Map([...recovery, ...redaction]) })
  assert.throws(() => requirePassedCi('HEAD', command), { message: 'NATIVE_NOTE_TRACER_HARNESS_NOT_APPROVED' })
})

test('refuses raw redaction assertions with the earlier recovery note suite', (t) => {
  const historicalFiles = harnessFilesFrom(t, recoveryNoteRevision, [browserSuites[0]])
  if (!historicalFiles) return
  const { command } = fixture({ historicalFiles })
  assert.throws(() => requirePassedCi('HEAD', command), { message: 'NATIVE_NOTE_TRACER_HARNESS_NOT_APPROVED' })
})

for (const step of ['text replacement', 'object editing', 'page management', 'field editing', 'redaction']) {
  test(`requires a successful expanded ${step} step`, () => {
    for (const conclusion of ['missing', 'skipped', 'failure']) {
      const { job, command } = fixture()
      const index = job.steps.findIndex(({ name }) => name.includes(step))
      assert.ok(index >= 0)
      if (conclusion === 'missing') job.steps.splice(index, 1)
      else job.steps[index].conclusion = conclusion
      assert.throws(() => requirePassedCi('HEAD', command), /JOB_NOT_PASSED/)
    }
  })
}

const refusals = [
  ['wrong origin', ({ state }) => { state.origin = 'https://github.com/another/flowpdf' }],
  ['unauthenticated', ({ state }) => { state.auth = false }],
  ['missing run', ({ state }) => { state.runs = [] }],
  ['wrong run SHA', ({ run }) => { run.head_sha = 'b'.repeat(40) }],
  ['wrong run repository', ({ run }) => { run.repository.full_name = 'another/flowpdf' }],
  ['modified workflow bytes', ({ state }) => { state.changedPath = '.github/workflows/pdf-native-note-tracer.yml' }],
  ['modified Cargo.toml', ({ state }) => { state.changedPath = 'Cargo.toml' }],
  ['modified crates/flow-core/Cargo.toml', ({ state }) => { state.changedPath = 'crates/flow-core/Cargo.toml' }],
  ['modified crates/flow-wasm/Cargo.toml', ({ state }) => { state.changedPath = 'crates/flow-wasm/Cargo.toml' }],
  ['modified build definition', ({ state }) => { state.changedPath = 'package.json' }],
  ['modified browser configuration', ({ state }) => { state.changedPath = 'vitest.config.ts' }],
  ['modified bilingual browser assertions', ({ state }) => { state.changedPath = 'web/tests/pdf-native-edit.browser.test.ts' }],
  ['modified expanded object assertions', ({ state }) => { state.changedPath = objectsSuite }],
  ['modified expanded page assertions', ({ state }) => { state.changedPath = 'web/tests/pdf-native-pages.browser.test.ts' }],
  ['modified expanded redaction assertions', ({ state }) => { state.changedPath = 'web/tests/pdf-native-redaction.browser.test.ts' }],
  ['modified built smoke assertions', ({ state }) => { state.changedPath = 'scripts/check-native-note-built.mjs' }],
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
