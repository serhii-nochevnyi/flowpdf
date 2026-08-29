import assert from 'node:assert/strict'
import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import {
  buildPhase2DependencyReport,
  verifyPhase2Dependencies,
} from './verify-phase2-dependencies.mjs'

const NOW = new Date('2026-08-29T12:00:00.000Z')
const REPOSITORY = 'https://github.com/example/demo'
const CRATE_REPOSITORY = 'https://github.com/example/unicode-demo'

function fixtureConfig() {
  return {
    schemaVersion: 1,
    timeoutMs: 50,
    retries: 0,
    npm: [{ name: 'demo', version: '1.9.0', repository: REPOSITORY }],
    crates: [{ name: 'unicode_demo', version: '2.3.0', repository: CRATE_REPOSITORY }],
    phase2: {
      policy: {
        minimumAgeDays: 30,
        minimumWeeklyDownloads: 1000,
        forbiddenLifecycleScripts: ['preinstall', 'install', 'postinstall'],
      },
      npm: [{
        name: 'demo',
        repository: REPOSITORY,
        candidates: ['2.0.0', '1.9.0'],
        lockIntent: { section: 'dependencies', version: '1.9.0' },
      }],
      crates: [{
        name: 'unicode_demo',
        repository: CRATE_REPOSITORY,
        candidates: ['2.3.0'],
        lockIntent: {
          manifest: 'crates/flow-core/Cargo.toml',
          version: '2.3.0',
          defaultFeatures: false,
          features: ['compiled_data'],
        },
      }],
    },
  }
}

function npmVersion(version) {
  return {
    name: 'demo',
    version,
    repository: { type: 'git', url: `${REPOSITORY}.git` },
    dist: {
      tarball: `https://registry.npmjs.org/demo/-/demo-${version}.tgz`,
      integrity: `sha512-${Buffer.from(`demo-${version}`).toString('base64')}`,
    },
    scripts: {},
  }
}

function makeFixtureFetch(mutate = () => {}) {
  const npmVersions = { '2.0.0': npmVersion('2.0.0'), '1.9.0': npmVersion('1.9.0') }
  const routes = new Map([
    ['https://registry.npmjs.org/demo', {
      name: 'demo',
      versions: npmVersions,
      time: { '2.0.0': '2026-08-20T00:00:00.000Z', '1.9.0': '2026-06-01T00:00:00.000Z' },
    }],
    ['https://registry.npmjs.org/demo/2.0.0', npmVersions['2.0.0']],
    ['https://registry.npmjs.org/demo/1.9.0', npmVersions['1.9.0']],
    ['https://api.npmjs.org/downloads/point/last-week/demo', {
      downloads: 50_000,
      period: 'last-week',
      start: '2026-08-22',
      end: '2026-08-28',
    }],
    ['https://api.github.com/repos/example/demo', {
      private: false,
      archived: false,
      html_url: REPOSITORY,
    }],
    ['https://crates.io/api/v1/crates/unicode_demo', {
      crate: {
        name: 'unicode_demo',
        repository: CRATE_REPOSITORY,
        created_at: '2020-01-01T00:00:00.000Z',
        recent_downloads: 25_000,
      },
      versions: [{
        num: '2.3.0',
        checksum: 'a'.repeat(64),
        yanked: false,
        created_at: '2026-08-20T00:00:00.000Z',
      }],
    }],
    ['https://crates.io/api/v1/crates/unicode_demo/2.3.0', {
      crate: { name: 'unicode_demo', repository: CRATE_REPOSITORY },
      version: {
        num: '2.3.0',
        checksum: 'a'.repeat(64),
        yanked: false,
        created_at: '2026-08-20T00:00:00.000Z',
      },
    }],
    ['https://api.github.com/repos/example/unicode-demo', {
      private: false,
      archived: false,
      html_url: CRATE_REPOSITORY,
    }],
  ])

  return async (url) => {
    const key = String(url)
    if (!routes.has(key)) return new Response('not found', { status: 404 })
    const value = structuredClone(routes.get(key))
    mutate(key, value)
    return Response.json(value)
  }
}

async function rejectsCode(config, mutate, code) {
  await assert.rejects(
    buildPhase2DependencyReport(config, { fetchImpl: makeFixtureFetch(mutate), now: NOW }),
    (error) => error.code === code,
  )
}

test('selects the first OK exact candidate and is deterministic apart from observation time', async () => {
  const config = fixtureConfig()
  const first = await buildPhase2DependencyReport(config, { fetchImpl: makeFixtureFetch(), now: NOW })
  const second = await buildPhase2DependencyReport(config, {
    fetchImpl: makeFixtureFetch(),
    now: new Date('2026-08-30T12:00:00.000Z'),
  })
  assert.equal(first.npm[0].version, '1.9.0')
  assert.equal(first.npm[0].candidateIndex, 1)
  assert.equal(first.npm[0].legitimacy.verdict, 'OK')
  assert.equal(first.crates[0].legitimacy.verdict, 'OK')
  delete first.observedAt
  delete second.observedAt
  assert.deepEqual(first, second)
})

test('rejects contradictory repository evidence', async () => {
  await rejectsCode(fixtureConfig(), (url, value) => {
    if (url.endsWith('/demo/2.0.0') || url.endsWith('/demo/1.9.0')) value.repository.url = 'https://github.com/impostor/demo.git'
  }, 'repository_mismatch')
})

test('rejects a non-canonical tarball', async () => {
  await rejectsCode(fixtureConfig(), (url, value) => {
    if (url === 'https://registry.npmjs.org/demo') value.versions['2.0.0'].dist.tarball = 'https://evil.invalid/demo.tgz'
  }, 'tarball_mismatch')
})

test('rejects missing or contradictory integrity', async () => {
  await rejectsCode(fixtureConfig(), (url, value) => {
    if (url === 'https://registry.npmjs.org/demo/2.0.0') value.dist.integrity = 'sha512-Y29udHJhZGljdGlvbg=='
  }, 'integrity_mismatch')
})

test('rejects deprecated candidates without falling through to an unapproved pin', async () => {
  await rejectsCode(fixtureConfig(), (url, value) => {
    if (url === 'https://registry.npmjs.org/demo') {
      value.versions['2.0.0'].deprecated = 'retired'
      value.versions['1.9.0'].deprecated = 'retired'
    }
    if (url === 'https://registry.npmjs.org/demo/2.0.0' || url === 'https://registry.npmjs.org/demo/1.9.0') value.deprecated = 'retired'
  }, 'legitimacy_not_ok')
})

test('rejects install lifecycle scripts', async () => {
  await rejectsCode(fixtureConfig(), (url, value) => {
    if (url === 'https://registry.npmjs.org/demo') {
      value.versions['2.0.0'].scripts.postinstall = 'node setup.js'
      value.versions['1.9.0'].scripts.postinstall = 'node setup.js'
    }
    if (url === 'https://registry.npmjs.org/demo/2.0.0' || url === 'https://registry.npmjs.org/demo/1.9.0') value.scripts.postinstall = 'node setup.js'
  }, 'legitimacy_not_ok')
})

test('rejects floating candidates before network access', async () => {
  const config = fixtureConfig()
  config.phase2.npm[0].candidates[0] = '^2.0.0'
  await assert.rejects(buildPhase2DependencyReport(config, {
    fetchImpl: async () => assert.fail('network should not be reached'),
    now: NOW,
  }), (error) => error.code === 'floating_version')
})

test('rejects stale lock intent before network access', async () => {
  const config = fixtureConfig()
  config.npm[0].version = '2.0.0'
  await assert.rejects(buildPhase2DependencyReport(config, {
    fetchImpl: async () => assert.fail('network should not be reached'),
    now: NOW,
  }), (error) => error.code === 'stale_lock_intent')
})

test('rejects a non-OK numeric legitimacy snapshot', async () => {
  await rejectsCode(fixtureConfig(), (url, value) => {
    if (url.includes('downloads/point')) value.downloads = 1
  }, 'legitimacy_not_ok')
})

test('writes an immutable blocker and removes stale success output on failure', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'flowpdf-phase2-provenance-'))
  const reportPath = path.join(directory, 'report.json')
  const blockerPath = path.join(directory, 'blocker.json')
  await writeFile(reportPath, '{"status":"stale"}\n')
  try {
    await assert.rejects(verifyPhase2Dependencies({
      config: fixtureConfig(),
      fetchImpl: makeFixtureFetch((url, value) => {
        if (url.includes('downloads/point')) value.downloads = 0
      }),
      now: NOW,
      reportPath,
      blockerPath,
    }))
    await assert.rejects(readFile(reportPath, 'utf8'), { code: 'ENOENT' })
    const blocker = JSON.parse(await readFile(blockerPath, 'utf8'))
    assert.equal(blocker.status, 'blocked')
    assert.equal(blocker.overrideAllowed, false)
    assert.equal(blocker.code, 'legitimacy_not_ok')
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test('writes success atomically and removes a prior blocker', async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'flowpdf-phase2-provenance-'))
  const reportPath = path.join(directory, 'report.json')
  const blockerPath = path.join(directory, 'blocker.json')
  await writeFile(blockerPath, '{"status":"blocked"}\n')
  try {
    const report = await verifyPhase2Dependencies({
      config: fixtureConfig(),
      fetchImpl: makeFixtureFetch(),
      now: NOW,
      reportPath,
      blockerPath,
    })
    assert.equal(report.status, 'success')
    assert.equal(JSON.parse(await readFile(reportPath, 'utf8')).lockIntentDigest, report.lockIntentDigest)
    await assert.rejects(readFile(blockerPath, 'utf8'), { code: 'ENOENT' })
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
})

test('verifier has no package-manager or shell execution path', async () => {
  const sourcePath = path.join(path.dirname(fileURLToPath(import.meta.url)), 'verify-phase2-dependencies.mjs')
  const source = await readFile(sourcePath, 'utf8')
  assert.doesNotMatch(source, /node:child_process|\b(?:spawn|exec|npm install|cargo add)\b/)
})
