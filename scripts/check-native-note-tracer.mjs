import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { delimiter, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../', import.meta.url))
const repository = 'serhii-nochevnyi/flowpdf'
const workflowFile = 'pdf-native-note-tracer.yml'
const workflowPath = `.github/workflows/${workflowFile}`
const workflowName = 'Native PDF note tracer'
const jobName = 'native-pdf-note-tracer'
const approvedHarness = [
  ['Cargo.toml', '57b1628917c2d4d4fdb08ae2b842f06dfbd1769729f1955cd31fde802e09bd38'],
  ['crates/flow-core/Cargo.toml', '5b54d3ade52e260aa29e31a86aef7123042d594fb0af04a6ba765b1c099d6d3e'],
  ['crates/flow-wasm/Cargo.toml', '1a76efa99818f2f129f40af76b952942dbc1bf4abb983daae2d1616874e05373'],
  ['package-lock.json', 'f1e5575f8687ea0fbf7f5abffddaa640116c8f24d09a724009d1b5210bceb3fd'],
  ['Cargo.lock', 'c04f2deac969c598caf8bc3b8e8b0ef23248115d3cf5492d06ac3ff366188be2'],
  ['rust-toolchain.toml', '683619e182e9d4e060bf754a8385eed1a1220cbc3e5a09e8c2dd22520af25dc6'],
  ['tsconfig.json', '8b048c299a7d6e709b15625b92a2bc4969f6891ee2eeb8176e6f05379dbb8a75'],
  ['tsconfig.web.json', 'c8fdaf6db3e965a6bdd4157bb88de4edb9c6a8976bf1b4233d08de1c7690c38f'],
  ['config/dependency-provenance.json', 'dbfb8e57d8f131f98b269729568e920b0e98a1d81379859cdd8d45091a5f45e0'],
  ['artifacts/provenance/phase1-dependencies.json', '7f3c1e134e955d2bfae6e696a2419164d92f0e19938793a7a61ccd19f5a3ba10'],
  ['scripts/build-web.mjs', 'f7c8e5fb15b200bad752c4c9d771a6b14b099d129f06bb51e5794595b7d49467'],
  ['scripts/verify-wasm-bindgen-tool.mjs', 'b80611196dcf834dd5ff2042bc58659539ae616529f14720d220eb740384c6c4'],
  ['scripts/node-version.mjs', '012d4782d3bef70f1cf7b4fa38eac1a550b8cb0b08bf7b8ebdce4f7a6de1a1db'],
  ['vite.config.ts', '2cbce239e60967b1e1418ad31be515168dcbdb61f0c62994103b926358b9652b'],
  ['vitest.config.ts', 'ec28afa609c9842929e91a4804c0b4b272e5bf0b7b7648b296d8c0b139cb60f8'],
  ['web/index.html', 'b822d2d54af4513d06170efce5b41fa450636a8e9ab85638c474d72b8e37f6c2'],
  ['web/editor.html', '1827eb7316907e5b0fabad0b7da31ea19172f65056d969ee05bbd9e01f8f5063'],
  ['scripts/check-native-note-built.mjs', 'aa9ae9e33f53a1ea00a92163ad9f2951d30705ce6892abd8f055b2877faed105'],
  ['scripts/serve-inspector.mjs', '07e3f482d39f79c3e4e3423e2627c5971477e492fc26855583ce37c77fe98376'],
]
const historicalWorkflow = '62f76deaa269fe69c7e44dcd54458f26ab4d65ea0b195fc1636a6c65788cd2d4'
const expandedWorkflow = '8d8e9d50c10fc6f3bed4803254abf99ac5955b445a81d699b785710268354a80'
const historicalPackage = 'a5686497377219cf6c5279e057fa831e445547226a39dc5418a31d5a4be9ab74'
const expandedPackage = 'a4496f8694df21f759c6bde4274ce4f849e057270871a85b84eb285c28bb4c04'
const historicalNote = ['web/tests/pdf-native-edit.browser.test.ts', 'e1b9dec38bbc3fe7dcc981c28c45576e22e419da4d8d230e5c739ccf6ff81410']
const expandedHarness = [
  ['web/tests/pdf-native-text.browser.test.ts', '46be62e7b0f31e30e877001aa714e6717eda4376585bd2fa5c340ec1140c533c'],
  ['web/tests/pdf-native-objects.browser.test.ts', 'a9ede30d753528f7b2e507b5f1403e96c93fb1b7028ecd5e8eab778795c943c4'],
  ['web/tests/pdf-native-pages.browser.test.ts', '251eba3f60794ee7d126d858f9377b752ceea1d152cbb5b4c9cfacaf53b51e3b'],
  ['web/tests/pdf-native-forms.browser.test.ts', '3d7f96b9c219f2a6c133fa4653caac5a1080045e3dcad7bd2a55bd1ed4e3ef52'],
  ['web/tests/pdf-native-redaction.browser.test.ts', '7c9ec2bb7fa67c6eee34969d7b4ef0e21352b886bad8a4d4a5cec1d95f5737e7'],
]
const reviewedBrowserSuites = [
  ['web/tests/pdf-native-edit.browser.test.ts', 'f544a59cf5b81b07317725b40171be0c3486f09a18eec352a9e8fe8364376228'],
  ['web/tests/pdf-native-text.browser.test.ts', '46be62e7b0f31e30e877001aa714e6717eda4376585bd2fa5c340ec1140c533c'],
  ['web/tests/pdf-native-objects.browser.test.ts', '5af47f1df548e19b4eb545797bbf7de339406133c31428a406cbb7ce736e1612'],
  ['web/tests/pdf-native-pages.browser.test.ts', '4d1e7b9af40bb99abcbbb8eb6ef275bcc32384141c233d914e4e91887fe783f5'],
  ['web/tests/pdf-native-forms.browser.test.ts', 'e3a8b28119e3a4d78215bd5694c1c740c6d8855d50bcc8dfdbc8d0319afacbcd'],
]
const reviewedPageBrowserSuites = reviewedBrowserSuites.map(([path, digest]) => [path,
  path === 'web/tests/pdf-native-pages.browser.test.ts' ? '75a10c2322c03833e9bf26edb683623779137e0aaa599875187a1a90c56e62e0' : digest,
])
const reviewedBrowserProfiles = [
  [...reviewedBrowserSuites,
    ['web/tests/pdf-native-redaction.browser.test.ts', 'f03b3854ae1f8b6459c400fc67dc5f43dc7185737eeb9eb3f7439ef104e0fb10']],
  [...reviewedBrowserSuites,
    ['web/tests/pdf-native-redaction.browser.test.ts', 'ba509b4cf12458746203d2b42d1cbaf955854226be3f0f5a27df91ee516abbd5']],
  [...reviewedPageBrowserSuites,
    ['web/tests/pdf-native-redaction.browser.test.ts', 'ba509b4cf12458746203d2b42d1cbaf955854226be3f0f5a27df91ee516abbd5']],
  [...reviewedPageBrowserSuites,
    ['web/tests/pdf-native-redaction.browser.test.ts', '8f51083c811286237e35d1437bbb369e5554e6ecea8a28938662ef278227dc95']],
  [...reviewedPageBrowserSuites,
    ['web/tests/pdf-native-redaction.browser.test.ts', '8adf4fa1bf071d7fa8c09b82faabfe98b30be7553aadae03d46f8af045aa99a1']],
  [...reviewedPageBrowserSuites.map(([path, digest]) => [path,
    path === 'web/tests/pdf-native-edit.browser.test.ts' ? '4cfff786a4a1caec82ef0879b90463070ff3e03ab57611a195a13c9df45a50b5' : digest,
  ]), ['web/tests/pdf-native-redaction.browser.test.ts', '8adf4fa1bf071d7fa8c09b82faabfe98b30be7553aadae03d46f8af045aa99a1']],
]
const expandedSteps = [
  'Run bilingual Chromium text replacement tracer',
  'Run bilingual Chromium object editing tracer',
  'Run bilingual Chromium page management tracer',
  'Run bilingual Chromium field editing tracer',
  'Run bilingual Chromium redaction tracer',
]
const requiredSteps = [
  'Verify exact checkout',
  'Build generated WASM and web output',
  'Run bilingual Chromium note tracer',
  'Bundle the production editor',
  'Run built-editor Chromium smoke',
  'Verify source checkout stayed unchanged',
]
const shaPattern = /^[a-f0-9]{40}$/
const apiTimeout = 20_000

function requireEvidence(condition, code) {
  if (!condition) throw new Error(code)
}

function execute(argv, { timeout = apiTimeout, env = process.env, raw = false } = {}) {
  const [command, ...args] = argv
  // Keep subprocess output out of gate diagnostics, including gh auth errors.
  const output = execFileSync(command, args, {
    cwd: root, env, encoding: 'utf8', timeout, maxBuffer: 8 * 1024 * 1024,
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  return raw ? output : output.trim()
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

  function harnessDigest(path) {
    const contents = call(command, ['git', 'show', `${sha}:${path}`],
      'NATIVE_NOTE_TRACER_HARNESS_UNAVAILABLE', { raw: true })
    return createHash('sha256').update(contents).digest('hex')
  }
  const workflowDigest = harnessDigest(workflowPath)
  const packageDigest = harnessDigest('package.json')
  const expanded = workflowDigest === expandedWorkflow
  requireEvidence((expanded || workflowDigest === historicalWorkflow) &&
    (packageDigest === historicalPackage || (expanded && packageDigest === expandedPackage)),
  'NATIVE_NOTE_TRACER_HARNESS_NOT_APPROVED')
  const steps = expanded ? [...requiredSteps.slice(0, 3), ...expandedSteps, ...requiredSteps.slice(3)] : requiredSteps
  for (const [path, digest] of approvedHarness) {
    requireEvidence(harnessDigest(path) === digest,
      'NATIVE_NOTE_TRACER_HARNESS_NOT_APPROVED')
  }
  const historicalBrowserHarness = expanded ? [historicalNote, ...expandedHarness] : [historicalNote]
  const observedBrowserHarness = new Map(historicalBrowserHarness.map(([path]) => [path, harnessDigest(path)]))
  const matchesBrowserHarness = (profile) => profile.every(([path, digest]) => observedBrowserHarness.get(path) === digest)
  requireEvidence(matchesBrowserHarness(historicalBrowserHarness) ||
    (expanded && packageDigest === expandedPackage && reviewedBrowserProfiles.some(matchesBrowserHarness)),
  'NATIVE_NOTE_TRACER_HARNESS_NOT_APPROVED')
  const { workflow_runs: runs, total_count: runCount } = get(`actions/workflows/${workflowFile}/runs?event=push&head_sha=${sha}&per_page=100`)
  requireEvidence(Array.isArray(runs) && runs.length > 0 && runs.length < 100 && runCount === runs.length &&
    runs.every((run) => Number.isSafeInteger(run.run_number)), 'NATIVE_NOTE_TRACER_RUN_UNAVAILABLE')
  const [run] = [...runs].sort((a, b) => b.run_number - a.run_number)
  requireEvidence(run?.head_sha === sha && run.repository?.full_name?.toLowerCase() === repository &&
    run.name === workflowName && run.path === workflowPath &&
    run.event === 'push' &&
    Number.isSafeInteger(run.id) && run.id > 0 &&
    Number.isSafeInteger(run.run_attempt) && run.run_attempt > 0 &&
    run.status === 'completed' && run.conclusion === 'success', 'NATIVE_NOTE_TRACER_RUN_NOT_PASSED')

  const { jobs, total_count: jobCount } = get(`actions/runs/${run.id}/attempts/${run.run_attempt}/jobs?per_page=100`)
  const matchingJobs = Array.isArray(jobs) && jobCount === jobs.length && jobs.length < 100
    ? jobs.filter((job) => job.name === jobName) : []
  const [job] = matchingJobs
  let stepNumber = 0
  const stepsPassed = Array.isArray(job?.steps) && steps.every((name) => {
    const matches = job.steps.filter((step) => step.name === name)
    const [step] = matches
    if (matches.length !== 1 || step.status !== 'completed' || step.conclusion !== 'success'
        || !Number.isSafeInteger(step.number) || step.number <= stepNumber) return false
    stepNumber = step.number
    return true
  })
  requireEvidence(matchingJobs.length === 1 && job.run_id === run.id &&
    (job.run_attempt === undefined || job.run_attempt === run.run_attempt) && job.head_sha === sha &&
    Number.isSafeInteger(job.id) && job.id > 0 &&
    typeof job.check_run_url === 'string' &&
    /^https:\/\/api\.github\.com\/repos\/serhii-nochevnyi\/flowpdf\/check-runs\/\d+$/.test(job.check_run_url) &&
    job.workflow_name === workflowName && job.labels?.includes('ubuntu-24.04') &&
    !job.labels.includes('self-hosted') && job.status === 'completed' && job.conclusion === 'success' && stepsPassed,
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
export const bundleArgv = ['npm', 'run', 'build:vite']
export const builtBrowserArgv = ['node', 'scripts/check-native-note-built.mjs']
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
  call(command, bundleArgv, 'NATIVE_NOTE_TRACER_BUNDLE_FAILED', { ...options, timeout: 5 * 60_000 })
  call(command, builtBrowserArgv, 'NATIVE_NOTE_TRACER_BUILT_BROWSER_FAILED', { ...options, timeout: 5 * 60_000 })
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
      console.log(JSON.stringify({ ...identity, sha, build: 'passed', browser: 'passed', builtBrowser: 'passed' }))
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
