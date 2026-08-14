import assert from 'node:assert/strict';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';

export function normalizeRepository(value) {
  if (typeof value !== 'string' || value.length === 0) return null;
  const normalized = value
    .replace(/^git\+/, '')
    // npm still exposes legacy git:// repository identities for otherwise
    // registry-hosted packages. This value is never fetched as Git; it is
    // canonicalized and then verified through the HTTPS GitHub API.
    .replace(/^git:\/\/github\.com\//i, 'https://github.com/');
  if (/%|\/(?:\.{1,2})(?:\/|$)/.test(normalized)) return null;
  let url;
  try {
    url = new URL(normalized);
  } catch {
    return null;
  }
  if (url.protocol !== 'https:' || url.hostname.toLowerCase() !== 'github.com' || url.port || url.username || url.password || url.search || url.hash) return null;
  const parts = url.pathname.split('/').filter(Boolean);
  if (parts.length < 2 || parts.some((part) => !/^[A-Za-z0-9_.-]+$/.test(part))) return null;
  const [owner, repositoryWithSuffix] = parts;
  const repository = repositoryWithSuffix.replace(/\.git$/, '');
  return repository ? `https://github.com/${owner}/${repository}` : null;
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

async function fetchText(url, { fetchImpl, timeoutMs, retries, packageName, check }) {
  let lastError;
  for (let attempt = 0; attempt <= retries; attempt += 1) {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), timeoutMs);
    try {
      const response = await fetchImpl(url, {
        headers: { accept: 'text/html', 'user-agent': 'flowpdf-provenance-verifier/1' },
        signal: controller.signal,
      });
      expect(response?.ok, packageName, check, `HTTP ${response?.status ?? 'network failure'}`);
      return await response.text();
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

async function verifyRepositoryPage(repositoryUrl, options, packageName) {
  const html = await fetchText(repositoryUrl, {
    ...options,
    packageName,
    check: 'upstream repository page',
  });
  const expectedNwo = new URL(repositoryUrl).pathname.slice(1);
  const nwo = html.match(/<meta\s+name="octolytics-dimension-repository_nwo"\s+content="([^"]+)"\s*\/?>/i)?.[1];
  const isPublic = html.match(/<meta\s+name="octolytics-dimension-repository_public"\s+content="([^"]+)"\s*\/?>/i)?.[1];
  expect(nwo?.toLowerCase() === expectedNwo.toLowerCase(), packageName, 'upstream repository page', 'repository identity metadata missing');
  expect(isPublic === 'true', packageName, 'upstream repository page', 'repository is not public');

  const embedded = html.match(/<script\s+type="application\/json"\s+data-target="react-app\.embeddedData">([\s\S]*?)<\/script>/i)?.[1];
  let repository;
  try {
    repository = JSON.parse(embedded).payload?.sidebarAbout?.repo;
  } catch {
    throw new ProvenanceError(packageName, 'upstream repository page', 'repository state metadata malformed');
  }
  expect(repository?.isPrivate === false, packageName, 'upstream repository page', 'repository is not public');
  expect(repository?.isArchived === false, packageName, 'upstream repository page', 'repository is archived');
  return { private: repository.isPrivate, archived: repository.isArchived };
}

async function verifyRepository(entry, registryRepository, options) {
  const actual = normalizeRepository(registryRepository);
  expect(actual === entry.repository, entry.name, 'repository', 'registry repository does not match allowlist');
  let repositoryPromise = options.repositoryCache.get(actual);
  if (!repositoryPromise) {
    repositoryPromise = fetchJson(githubApiUrl(actual), {
      ...options,
      packageName: entry.name,
      check: 'upstream repository',
    });
    options.repositoryCache.set(actual, repositoryPromise);
  }
  let repository;
  try {
    repository = await repositoryPromise;
  } catch (error) {
    // GitHub's unauthenticated REST limit is shared by the network egress.
    // On a 403, verify the same public/non-archived invariants from GitHub's
    // official repository page. Its identity and embedded state are required
    // exactly; any markup drift remains a fail-closed error.
    if (!(error instanceof ProvenanceError) || error.reason !== 'HTTP 403') throw error;
    let pagePromise = options.repositoryPageCache.get(actual);
    if (!pagePromise) {
      pagePromise = verifyRepositoryPage(actual, options, entry.name);
      options.repositoryPageCache.set(actual, pagePromise);
    }
    repository = await pagePromise;
  }
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
    ecosystem: 'crates.io', name: entry.name, version: entry.version, kind: entry.kind ?? 'dependency', repository,
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
  const repository = await verifyRepository(entry, data.repository?.url ?? data.repository, options);
  // The exact-version endpoint intentionally omits the package-level `time`
  // map. Resolve the timestamp from the authoritative package document while
  // keeping identity, repository and integrity checks bound to the exact
  // version response above.
  const packageMetadata = await fetchJson(`https://registry.npmjs.org/${encoded}`, {
    ...options,
    packageName: entry.name,
    check: 'npm publish metadata',
  });
  expect(packageMetadata.versions?.[entry.version]?.version === entry.version, entry.name, 'publish metadata', 'exact version missing from npm package document');
  const publishedAt = packageMetadata.time?.[entry.version];
  expect(typeof publishedAt === 'string', entry.name, 'publish metadata', 'publish timestamp missing');
  return {
    ecosystem: 'npm', name: entry.name, version: entry.version, kind: entry.kind ?? 'dependency', repository,
    publishedAt, integrity, source: `https://registry.npmjs.org/${encoded}/${entry.version}`, tarball,
  };
}

export async function verifyManifest({ config, fetchImpl = globalThis.fetch }) {
  expect(config?.schemaVersion === 1, 'manifest', 'schema', 'unsupported schema version');
  const options = {
    fetchImpl,
    timeoutMs: config.timeoutMs,
    retries: config.retries,
    // wasm-bindgen and its CLI intentionally share one upstream. Cache the
    // official repository response so a run consumes one bounded API request
    // per canonical identity and cannot amplify rate limits through aliases.
    repositoryCache: new Map(),
    repositoryPageCache: new Map(),
  };
  const crates = await Promise.all(config.crates.map((entry) => verifyCrate(entry, options)));
  const npm = await Promise.all(config.npm.map((entry) => verifyNpm(entry, options)));
  return { schemaVersion: 1, status: 'success', crates, npm };
}

export async function writeVerificationOutcome({ config, reportPath, blockerPath, fetchImpl }) {
  try {
    const report = await verifyManifest({ config, fetchImpl });
    await mkdir(dirname(reportPath), { recursive: true });
    await writeFile(reportPath, `${JSON.stringify(report, null, 2)}\n`);
    await rm(blockerPath, { force: true });
    return { ok: true, report };
  } catch (error) {
    const blocker = {
      schemaVersion: 1, status: 'blocked', package: error.packageName ?? 'manifest',
      check: error.check ?? 'unexpected error', reason: error.reason ?? 'verifier failed',
      timestamp: new Date().toISOString(),
    };
    await mkdir(dirname(blockerPath), { recursive: true });
    await rm(reportPath, { force: true });
    await writeFile(blockerPath, `${JSON.stringify(blocker, null, 2)}\n`);
    return { ok: false, blocker };
  }
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
    ['https://registry.npmjs.org/vitest', { versions: { '4.1.6': { version: '4.1.6' } }, time: { '4.1.6': '2026-01-01T00:00:00Z' } }],
    ['https://api.github.com/repos/serde-rs/serde', { private: false, archived: false }],
    ['https://api.github.com/repos/vitest-dev/vitest', { private: false, archived: false }],
  ]);
  const fetchImpl = async (url) => new Response(JSON.stringify(responses.get(url)), { status: responses.has(url) ? 200 : 404 });
  const report = await verifyManifest({ config, fetchImpl });
  assert.equal(report.status, 'success');
  assert.equal(report.crates[0].checksum.length, 64);
  assert.match(report.npm[0].integrity, /^sha512-/);
  const directory = await mkdtemp(join(tmpdir(), 'flowpdf-provenance-'));
  const reportPath = join(directory, 'report.json');
  const blockerPath = join(directory, 'blocker.json');
  await writeFile(blockerPath, 'stale blocker');
  const result = await writeVerificationOutcome({ config, fetchImpl, reportPath, blockerPath });
  assert.equal(result.ok, true);
  await assert.doesNotReject(() => readFile(reportPath));
  await assert.rejects(() => readFile(blockerPath));
});

test('normalizes git+https repository URLs', () => {
  assert.equal(
    normalizeRepository('git+https://github.com/example/repository.git'),
    'https://github.com/example/repository',
  );
  assert.equal(
    normalizeRepository('https://github.com/wasm-bindgen/wasm-bindgen/tree/master/crates/cli'),
    'https://github.com/wasm-bindgen/wasm-bindgen',
  );
  assert.equal(
    normalizeRepository('git://github.com/dumbmatter/fakeIndexedDB.git'),
    'https://github.com/dumbmatter/fakeIndexedDB',
  );
});

test('rejects unsafe or noncanonical repository URLs', () => {
  for (const url of [
    'http://github.com/serde-rs/serde',
    'https://gitlab.com/serde-rs/serde',
    'https://github.com/serde-rs',
    'https://user:password@github.com/serde-rs/serde',
    'https://github.com/serde-rs/../serde',
    'https://github.com/serde-rs/%2e%2e/serde',
  ]) {
    assert.equal(normalizeRepository(url), null, url);
  }
});

test('fails over from a rate-limited GitHub API to strict official page metadata', async () => {
  const config = {
    schemaVersion: 1, timeoutMs: 10, retries: 0, npm: [],
    crates: [{ name: 'serde', version: '1.0.228', repository: 'https://github.com/serde-rs/serde' }],
  };
  const page = (archived) => `
    <meta name="octolytics-dimension-repository_nwo" content="serde-rs/serde" />
    <meta name="octolytics-dimension-repository_public" content="true" />
    <script type="application/json" data-target="react-app.embeddedData">{"payload":{"sidebarAbout":{"repo":{"isPrivate":false,"isArchived":${archived}}}}}</script>`;
  const fetchImpl = async (url) => {
    if (url.startsWith('https://crates.io/')) return new Response(JSON.stringify({ version: { num: '1.0.228', yanked: false, checksum: 'a'.repeat(64), created_at: '2026-01-01T00:00:00Z', repository: 'https://github.com/serde-rs/serde' } }), { status: 200 });
    if (url.startsWith('https://api.github.com/')) return new Response('', { status: 403 });
    return new Response(page(false), { status: 200, headers: { 'content-type': 'text/html' } });
  };
  assert.equal((await verifyManifest({ config, fetchImpl })).status, 'success');
  const archivedFetch = async (url) => {
    if (url.startsWith('https://crates.io/')) return fetchImpl(url);
    if (url.startsWith('https://api.github.com/')) return new Response('', { status: 403 });
    return new Response(page(true), { status: 200, headers: { 'content-type': 'text/html' } });
  };
  await assert.rejects(() => verifyManifest({ config, fetchImpl: archivedFetch }), /archived/);
});

test('fails closed for every required negative provenance invariant', async () => {
  const base = {
    schemaVersion: 1, timeoutMs: 10, retries: 0, npm: [],
    crates: [{ name: 'serde', version: '1.0.228', repository: 'https://github.com/serde-rs/serde' }],
  };
  const yanked = async () => new Response(JSON.stringify({ version: { num: '1.0.228', yanked: true, checksum: 'a'.repeat(64), created_at: '2026-01-01T00:00:00Z', repository: 'https://github.com/serde-rs/serde' } }), { status: 200 });
  await assert.rejects(() => verifyManifest({ config: base, fetchImpl: yanked }), /release state/);
  const missingChecksum = async () => new Response(JSON.stringify({ version: { num: '1.0.228', yanked: false, created_at: '2026-01-01T00:00:00Z', repository: 'https://github.com/serde-rs/serde' } }), { status: 200 });
  await assert.rejects(() => verifyManifest({ config: base, fetchImpl: missingChecksum }), /checksum/);
  const npmConfig = { ...base, crates: [], npm: [{ name: 'vitest', version: '4.1.6', repository: 'https://github.com/vitest-dev/vitest' }] };
  const missingIntegrity = async () => new Response(JSON.stringify({ name: 'vitest', version: '4.1.6', time: { '4.1.6': '2026-01-01T00:00:00Z' }, repository: { url: 'https://github.com/vitest-dev/vitest' }, dist: { tarball: 'https://registry.npmjs.org/vitest/-/x.tgz' } }), { status: 200 });
  await assert.rejects(() => verifyManifest({ config: npmConfig, fetchImpl: missingIntegrity }), /integrity/);
  const nonRegistryTarball = async () => new Response(JSON.stringify({ name: 'vitest', version: '4.1.6', time: { '4.1.6': '2026-01-01T00:00:00Z' }, repository: { url: 'https://github.com/vitest-dev/vitest' }, dist: { integrity: 'sha512-test', tarball: 'https://example.invalid/vitest.tgz' } }), { status: 200 });
  await assert.rejects(() => verifyManifest({ config: npmConfig, fetchImpl: nonRegistryTarball }), /tarball/);
  const repositoryMismatch = async () => new Response(JSON.stringify({ name: 'vitest', version: '4.1.6', time: { '4.1.6': '2026-01-01T00:00:00Z' }, repository: { url: 'https://github.com/example/untrusted' }, dist: { integrity: 'sha512-test', tarball: 'https://registry.npmjs.org/vitest/-/x.tgz' } }), { status: 200 });
  await assert.rejects(() => verifyManifest({ config: npmConfig, fetchImpl: repositoryMismatch }), /repository/);
  const deprecated = async () => new Response(JSON.stringify({ name: 'vitest', version: '4.1.6', deprecated: 'no longer supported', time: { '4.1.6': '2026-01-01T00:00:00Z' }, repository: { url: 'https://github.com/vitest-dev/vitest' }, dist: { integrity: 'sha512-test', tarball: 'https://registry.npmjs.org/vitest/-/x.tgz' } }), { status: 200 });
  await assert.rejects(() => verifyManifest({ config: npmConfig, fetchImpl: deprecated }), /release state/);
  const malformed = async () => new Response('{', { status: 200 });
  await assert.rejects(() => verifyManifest({ config: base, fetchImpl: malformed }), /malformed JSON/);
  const httpFailure = async () => new Response('', { status: 503 });
  await assert.rejects(() => verifyManifest({ config: base, fetchImpl: httpFailure }), /HTTP 503/);
  const timeout = async (_url, options) => new Promise((_, reject) => options.signal.addEventListener('abort', () => reject(new DOMException('aborted', 'AbortError'))));
  await assert.rejects(() => verifyManifest({ config: { ...base, timeoutMs: 1 }, fetchImpl: timeout }), /timeout/);
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
  const result = await writeVerificationOutcome({ config, reportPath, blockerPath });
  if (!result.ok) process.exitCode = 1;
}
