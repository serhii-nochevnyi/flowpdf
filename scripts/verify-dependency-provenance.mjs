import assert from 'node:assert/strict';
import { mkdir, readFile, rm, writeFile } from 'node:fs/promises';
import { dirname } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

export function normalizeRepository(value) {
  if (typeof value !== 'string' || value.length === 0) return null;
  const normalized = value
    .replace(/^git\+/, '')
    .replace(/^git:\/\//, 'https://')
    .replace(/\.git(?:#.*)?$/, '')
    .replace(/\/$/, '');
  return /^https:\/\/(?:www\.)?github\.com\/[\w.-]+\/[\w.-]+$/i.test(normalized)
    ? normalized.replace(/^https:\/\/www\./, 'https://')
    : null;
}

class ProvenanceError extends Error {
  constructor(packageName, check, reason) {
    super(`${packageName}: ${check}: ${reason}`);
    this.packageName = packageName;
    this.check = check;
    this.reason = reason;
  }
}

function expect(condition, packageName, check, reason) {
  if (!condition) throw new ProvenanceError(packageName, check, reason);
}

function githubApiUrl(repository) {
  const parsed = new URL(repository);
  return `https://api.github.com/repos${parsed.pathname}`;
}

async function fetchJson(url, { fetchImpl, timeoutMs, retries, packageName, check }) {
  let lastError;
  for (let attempt = 0; attempt <= retries; attempt += 1) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);
    try {
      const response = await fetchImpl(url, {
        headers: { accept: 'application/json', 'user-agent': 'flowpdf-provenance-verifier/1' },
        signal: controller.signal,
      });
      expect(response?.ok, packageName, check, `HTTP ${response?.status ?? 'network failure'}`);
      try {
        return await response.json();
      } catch {
        throw new ProvenanceError(packageName, check, 'malformed JSON response');
      }
    } catch (error) {
      lastError = error instanceof ProvenanceError
        ? error
        : new ProvenanceError(packageName, check, error?.name === 'AbortError' ? 'timeout' : 'network failure');
      if (error instanceof ProvenanceError || attempt === retries) break;
    } finally {
      clearTimeout(timer);
    }
  }
  throw lastError;
}

async function verifyRepository(entry, registryRepository, options) {
  const actual = normalizeRepository(registryRepository);
  expect(actual === entry.repository, entry.name, 'repository', 'registry repository does not match allowlist');
  const repository = await fetchJson(githubApiUrl(actual), {
    ...options,
    packageName: entry.name,
    check: 'upstream repository',
  });
  expect(repository.private === false, entry.name, 'upstream repository', 'repository is not public');
  expect(repository.archived === false, entry.name, 'upstream repository', 'repository is archived');
  return actual;
}

async function verifyCrate(entry, options) {
  const data = await fetchJson(`https://crates.io/api/v1/crates/${encodeURIComponent(entry.name)}/${entry.version}`, {
    ...options,
    packageName: entry.name,
    check: 'crates.io release',
  });
  const version = data.version;
  expect(version && version.num === entry.version, entry.name, 'version', 'exact version missing from crates.io');
  expect(version.yanked === false, entry.name, 'release state', 'release is yanked');
  expect(typeof version.checksum === 'string' && /^[a-f0-9]{64}$/i.test(version.checksum), entry.name, 'checksum', 'crates.io checksum missing');
  expect(typeof version.created_at === 'string', entry.name, 'publish metadata', 'publish timestamp missing');
  const repository = await verifyRepository(entry, version.repository ?? data.crate?.repository, options);
  return {
    ecosystem: 'crates.io', name: entry.name, version: entry.version, repository,
    publishedAt: version.created_at, checksum: version.checksum,
    source: `https://crates.io/api/v1/crates/${encodeURIComponent(entry.name)}/${entry.version}`,
  };
}

async function verifyNpm(entry, options) {
  const encoded = encodeURIComponent(entry.name);
  const data = await fetchJson(`https://registry.npmjs.org/${encoded}/${entry.version}`, {
    ...options,
    packageName: entry.name,
    check: 'npm release',
  });
  expect(data.name === entry.name && data.version === entry.version, entry.name, 'version', 'exact version missing from npm registry');
  expect(!data.deprecated, entry.name, 'release state', 'release is deprecated');
  const integrity = data.dist?.integrity;
  expect(typeof integrity === 'string' && integrity.startsWith('sha512-'), entry.name, 'integrity', 'sha512 integrity missing');
  const tarball = data.dist?.tarball;
  expect(typeof tarball === 'string' && new URL(tarball).hostname === 'registry.npmjs.org', entry.name, 'tarball', 'tarball is not registry-hosted');
  const publishedAt = data.time?.[entry.version];
  expect(typeof publishedAt === 'string', entry.name, 'publish metadata', 'publish timestamp missing');
  const repository = await verifyRepository(entry, data.repository?.url ?? data.repository, options);
  return {
    ecosystem: 'npm', name: entry.name, version: entry.version, repository,
    publishedAt, integrity, source: `https://registry.npmjs.org/${encoded}/${entry.version}`, tarball,
  };
}

export async function verifyManifest({ config, fetchImpl = globalThis.fetch }) {
  expect(config?.schemaVersion === 1, 'manifest', 'schema', 'unsupported schema version');
  const options = { fetchImpl, timeoutMs: config.timeoutMs, retries: config.retries };
  const crates = await Promise.all(config.crates.map((entry) => verifyCrate(entry, options)));
  const npm = await Promise.all(config.npm.map((entry) => verifyNpm(entry, options)));
  return { schemaVersion: 1, status: 'success', verifiedAt: new Date().toISOString(), crates, npm };
}

test('accepts an allowlisted stable crate and npm release with complete provenance', async () => {
  const config = {
    schemaVersion: 1, timeoutMs: 10, retries: 0,
    crates: [{ name: 'serde', version: '1.0.228', repository: 'https://github.com/serde-rs/serde' }],
    npm: [{ name: 'vitest', version: '4.1.6', repository: 'https://github.com/vitest-dev/vitest' }],
  };
  const responses = new Map([
    ['https://crates.io/api/v1/crates/serde/1.0.228', { version: { num: '1.0.228', yanked: false, checksum: 'a'.repeat(64), created_at: '2026-01-01T00:00:00Z', repository: 'https://github.com/serde-rs/serde' } }],
    ['https://registry.npmjs.org/vitest/4.1.6', { name: 'vitest', version: '4.1.6', time: { '4.1.6': '2026-01-01T00:00:00Z' }, repository: { url: 'git+https://github.com/vitest-dev/vitest.git' }, dist: { integrity: 'sha512-test', tarball: 'https://registry.npmjs.org/vitest/-/vitest-4.1.6.tgz' } }],
    ['https://api.github.com/repos/serde-rs/serde', { private: false, archived: false }],
    ['https://api.github.com/repos/vitest-dev/vitest', { private: false, archived: false }],
  ]);
  const fetchImpl = async (url) => new Response(JSON.stringify(responses.get(url)), { status: responses.has(url) ? 200 : 404 });
  const report = await verifyManifest({ config, fetchImpl });
  assert.equal(report.status, 'success');
  assert.equal(report.crates[0].checksum.length, 64);
  assert.match(report.npm[0].integrity, /^sha512-/);
});

test('normalizes git+https repository URLs', () => {
  assert.equal(
    normalizeRepository('git+https://github.com/example/repository.git'),
    'https://github.com/example/repository',
  );
});

test('fails closed for yanked crates, missing npm integrity, and repository mismatch', async () => {
  const base = {
    schemaVersion: 1, timeoutMs: 10, retries: 0, npm: [],
    crates: [{ name: 'serde', version: '1.0.228', repository: 'https://github.com/serde-rs/serde' }],
  };
  const yanked = async () => new Response(JSON.stringify({ version: { num: '1.0.228', yanked: true, checksum: 'a'.repeat(64), created_at: '2026-01-01T00:00:00Z', repository: 'https://github.com/serde-rs/serde' } }), { status: 200 });
  await assert.rejects(() => verifyManifest({ config: base, fetchImpl: yanked }), /release state/);
  const npmConfig = { ...base, crates: [], npm: [{ name: 'vitest', version: '4.1.6', repository: 'https://github.com/vitest-dev/vitest' }] };
  const missingIntegrity = async () => new Response(JSON.stringify({ name: 'vitest', version: '4.1.6', time: { '4.1.6': '2026-01-01T00:00:00Z' }, repository: { url: 'https://github.com/vitest-dev/vitest' }, dist: { tarball: 'https://registry.npmjs.org/vitest/-/x.tgz' } }), { status: 200 });
  await assert.rejects(() => verifyManifest({ config: npmConfig, fetchImpl: missingIntegrity }), /integrity/);
});

if (process.argv.includes('--config')) {
  const input = process.argv.slice(2);
  const valueAfter = (flag) => input[input.indexOf(flag) + 1];
  const configPath = valueAfter('--config');
  const reportPath = valueAfter('--report');
  const blockerPath = valueAfter('--blocker');
  if (!configPath || !reportPath || !blockerPath) {
    throw new Error('Usage: --config <path> --report <path> --blocker <path>');
  }
  const config = JSON.parse(await readFile(configPath, 'utf8'));
  try {
    const report = await verifyManifest({ config });
    await mkdir(dirname(reportPath), { recursive: true });
    await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
    await rm(blockerPath, { force: true });
  } catch (error) {
    const blocker = {
      schemaVersion: 1,
      status: 'blocked',
      package: error.packageName ?? 'manifest',
      check: error.check ?? 'unexpected error',
      reason: error.reason ?? 'verifier failed',
      timestamp: new Date().toISOString(),
    };
    await mkdir(dirname(blockerPath), { recursive: true });
    await rm(reportPath, { force: true });
    await writeFile(blockerPath, `${JSON.stringify(blocker, null, 2)}\n`);
    process.exitCode = 1;
  }
}
