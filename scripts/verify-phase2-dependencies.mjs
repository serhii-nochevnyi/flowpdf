#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const DEFAULT_CONFIG = path.join(ROOT, 'config/dependency-provenance.json')
const DEFAULT_REPORT = path.join(ROOT, 'artifacts/provenance/phase2-dependencies.json')
const DEFAULT_BLOCKER = path.join(ROOT, 'artifacts/provenance/phase2-dependency-blocker.json')
const EXACT_VERSION = /^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/

export class EvidenceError extends Error {
  constructor(code, message) {
    super(`${code}: ${message}`)
    this.name = 'EvidenceError'
    this.code = code
  }
}

function requireFact(condition, code, message) {
  if (!condition) throw new EvidenceError(code, message)
}

function npmPath(name) {
  return encodeURIComponent(name)
}

function repositoryUrl(value) {
  const raw = typeof value === 'string' ? value : value?.url
  if (!raw) return null
  return raw
    .replace(/^git\+/, '')
    .replace(/^git:\/\//, 'https://')
    .replace(/^ssh:\/\/git@github\.com\//, 'https://github.com/')
    .replace(/^git@github\.com:/, 'https://github.com/')
    .replace(/\.git(?:#.*)?$/, '')
    .replace(/\/$/, '')
}

function githubSlug(repository) {
  const normalized = repositoryUrl(repository)
  const match = normalized?.match(/^https:\/\/github\.com\/([^/]+)\/([^/]+)$/i)
  requireFact(match, 'repository_not_github', `expected an exact GitHub repository URL, received ${normalized ?? 'none'}`)
  return `${match[1]}/${match[2]}`
}

function canonicalNpmTarball(name, version) {
  const leaf = name.includes('/') ? name.slice(name.lastIndexOf('/') + 1) : name
  return `https://registry.npmjs.org/${name}/-/${leaf}-${version}.tgz`
}

function exactVersion(value, label) {
  requireFact(typeof value === 'string' && EXACT_VERSION.test(value), 'floating_version', `${label} must be an exact version, received ${String(value)}`)
}

function isoDate(value, label) {
  const date = new Date(value)
  requireFact(Number.isFinite(date.getTime()), 'missing_publish_time', `${label} has no valid publication timestamp`)
  return date
}

function ageDays(from, now) {
  return (now.getTime() - from.getTime()) / 86_400_000
}

function stableDigest(value) {
  return createHash('sha256').update(JSON.stringify(value)).digest('hex')
}

async function fetchResponse(fetchImpl, url, { timeoutMs, retries, accept = 'application/json' }) {
  let lastError
  for (let attempt = 0; attempt <= retries; attempt += 1) {
    const controller = new AbortController()
    const timer = setTimeout(() => controller.abort(), timeoutMs)
    try {
      const response = await fetchImpl(url, {
        signal: controller.signal,
        headers: {
          accept,
          'user-agent': 'flowpdf-dependency-verifier/2',
        },
      })
      clearTimeout(timer)
      return response
    } catch (error) {
      clearTimeout(timer)
      lastError = error
    }
  }
  throw new EvidenceError('metadata_unavailable', `${url}: ${lastError?.message ?? 'request failed'}`)
}

async function fetchJson(fetchImpl, url, options) {
  const response = await fetchResponse(fetchImpl, url, options)
  requireFact(response.ok, 'metadata_unavailable', `${url} returned HTTP ${response.status}`)
  try {
    return await response.json()
  } catch (error) {
    throw new EvidenceError('invalid_metadata', `${url}: ${error.message}`)
  }
}

async function verifyGithubRepository(fetchImpl, repository, options) {
  const slug = githubSlug(repository)
  const apiUrl = `https://api.github.com/repos/${slug}`
  const response = await fetchResponse(fetchImpl, apiUrl, options)

  if (response.ok) {
    const data = await response.json()
    requireFact(data.private === false, 'repository_not_public', `${repository} is not public`)
    requireFact(data.archived === false, 'repository_archived', `${repository} is archived`)
    const canonical = repositoryUrl(data.html_url)
    requireFact(canonical?.toLowerCase() === repositoryUrl(repository).toLowerCase(), 'repository_mismatch', `${repository} resolves to ${canonical}`)
    return { repository: canonical, source: apiUrl, archived: false, public: true }
  }

  requireFact(response.status === 403 || response.status === 429, 'repository_unavailable', `${apiUrl} returned HTTP ${response.status}`)
  const pageUrl = `https://github.com/${slug}`
  const page = await fetchResponse(fetchImpl, pageUrl, { ...options, accept: 'text/html' })
  requireFact(page.ok, 'repository_unavailable', `${pageUrl} returned HTTP ${page.status}`)
  const body = await page.text()
  requireFact(!/This repository was archived by the owner/i.test(body), 'repository_archived', `${repository} is archived`)
  requireFact(/<title>[^<]+[·-]\s*GitHub<\/title>/i.test(body), 'repository_unverified', `${pageUrl} did not return a recognizable repository page`)
  return { repository: repositoryUrl(repository), source: pageUrl, archived: false, public: true }
}

function validateConfiguration(config) {
  requireFact(config?.schemaVersion === 1, 'invalid_config', 'schemaVersion must be 1')
  requireFact(Array.isArray(config.phase2?.npm) && Array.isArray(config.phase2?.crates), 'invalid_config', 'phase2 npm and crates lists are required')
  const policy = config.phase2.policy
  requireFact(Number.isFinite(policy?.minimumAgeDays) && policy.minimumAgeDays >= 0, 'invalid_policy', 'minimumAgeDays must be numeric')
  requireFact(Number.isFinite(policy?.minimumWeeklyDownloads) && policy.minimumWeeklyDownloads >= 0, 'invalid_policy', 'minimumWeeklyDownloads must be numeric')
  requireFact(Array.isArray(policy?.forbiddenLifecycleScripts), 'invalid_policy', 'forbiddenLifecycleScripts must be an array')

  for (const [kind, entries] of [['npm', config.phase2.npm], ['crates', config.phase2.crates]]) {
    for (const entry of entries) {
      requireFact(entry.name && entry.repository && Array.isArray(entry.candidates) && entry.candidates.length > 0, 'invalid_config', `${kind}:${entry.name ?? 'unknown'} is incomplete`)
      entry.candidates.forEach((candidate) => exactVersion(candidate, `${kind}:${entry.name} candidate`))
      exactVersion(entry.lockIntent?.version, `${kind}:${entry.name} lock intent`)
      requireFact(entry.candidates.includes(entry.lockIntent.version), 'stale_lock_intent', `${kind}:${entry.name} lock intent ${entry.lockIntent.version} is not an admitted candidate`)
      const pinned = config[kind]?.find((item) => item.name === entry.name)
      requireFact(pinned?.version === entry.lockIntent.version, 'stale_lock_intent', `${kind}:${entry.name} top-level pin does not match ${entry.lockIntent.version}`)
      requireFact(repositoryUrl(pinned.repository) === repositoryUrl(entry.repository), 'repository_mismatch', `${kind}:${entry.name} top-level repository differs from phase2 intent`)
    }
  }
}

async function inspectNpmCandidate(entry, version, context) {
  const encodedName = npmPath(entry.name)
  const packageUrl = `https://registry.npmjs.org/${encodedName}`
  const versionUrl = `${packageUrl}/${encodeURIComponent(version)}`
  const downloadsUrl = `https://api.npmjs.org/downloads/point/last-week/${encodedName}`
  const [packageDoc, exactDoc, downloads] = await Promise.all([
    fetchJson(context.fetchImpl, packageUrl, context),
    fetchJson(context.fetchImpl, versionUrl, context),
    fetchJson(context.fetchImpl, downloadsUrl, context),
  ])
  const indexed = packageDoc.versions?.[version]
  requireFact(indexed && exactDoc?.version === version, 'missing_exact_version', `npm:${entry.name}@${version} is absent from official metadata`)
  const expectedRepository = repositoryUrl(entry.repository)
  const indexedRepository = repositoryUrl(indexed.repository)
  const exactRepository = repositoryUrl(exactDoc.repository)
  requireFact(indexedRepository === expectedRepository && exactRepository === expectedRepository, 'repository_mismatch', `npm:${entry.name}@${version} repository evidence is contradictory`)

  const expectedTarball = canonicalNpmTarball(entry.name, version)
  requireFact(indexed.dist?.tarball === expectedTarball && exactDoc.dist?.tarball === expectedTarball, 'tarball_mismatch', `npm:${entry.name}@${version} has a non-canonical tarball`)
  requireFact(/^sha512-[A-Za-z0-9+/]+={0,2}$/.test(indexed.dist?.integrity ?? ''), 'integrity_missing', `npm:${entry.name}@${version} has no sha512 integrity`)
  requireFact(indexed.dist.integrity === exactDoc.dist?.integrity, 'integrity_mismatch', `npm:${entry.name}@${version} integrity endpoints disagree`)
  requireFact((indexed.deprecated ?? null) === (exactDoc.deprecated ?? null), 'deprecation_mismatch', `npm:${entry.name}@${version} deprecation endpoints disagree`)

  const lifecycle = {}
  for (const key of context.policy.forbiddenLifecycleScripts) {
    const indexedValue = indexed.scripts?.[key] ?? null
    const exactValue = exactDoc.scripts?.[key] ?? null
    requireFact(indexedValue === exactValue, 'lifecycle_mismatch', `npm:${entry.name}@${version} ${key} endpoints disagree`)
    lifecycle[key] = exactValue
  }
  const publishedAt = isoDate(packageDoc.time?.[version], `npm:${entry.name}@${version}`)
  requireFact(Number.isFinite(downloads.downloads), 'missing_download_snapshot', `npm:${entry.name}@${version} has no numeric download snapshot`)
  const repository = await verifyGithubRepository(context.fetchImpl, entry.repository, context)
  const reasons = []
  if (exactDoc.deprecated) reasons.push(`deprecated: ${exactDoc.deprecated}`)
  for (const [key, value] of Object.entries(lifecycle)) if (value) reasons.push(`forbidden lifecycle script: ${key}`)
  if (ageDays(publishedAt, context.now) < context.policy.minimumAgeDays) reasons.push(`release age below ${context.policy.minimumAgeDays} days`)
  if (downloads.downloads < context.policy.minimumWeeklyDownloads) reasons.push(`weekly downloads below ${context.policy.minimumWeeklyDownloads}`)

  return {
    accepted: reasons.length === 0,
    reasons,
    evidence: {
      name: entry.name,
      version,
      repository: repository.repository,
      publishedAt: publishedAt.toISOString(),
      tarball: expectedTarball,
      integrity: exactDoc.dist.integrity,
      deprecated: exactDoc.deprecated ?? null,
      lifecycle,
      numericSnapshot: {
        metric: 'weeklyDownloads',
        value: downloads.downloads,
        period: downloads.period ?? 'last-week',
        start: downloads.start ?? null,
        end: downloads.end ?? null,
        source: downloadsUrl,
      },
      sources: { package: packageUrl, version: versionUrl, repository: repository.source },
      legitimacy: { verdict: reasons.length === 0 ? 'OK' : 'SUS', reasons },
      lockIntent: entry.lockIntent,
    },
  }
}

async function inspectCrateCandidate(entry, version, context) {
  const packageUrl = `https://crates.io/api/v1/crates/${encodeURIComponent(entry.name)}`
  const versionUrl = `${packageUrl}/${encodeURIComponent(version)}`
  const [packageDoc, exactDoc] = await Promise.all([
    fetchJson(context.fetchImpl, packageUrl, context),
    fetchJson(context.fetchImpl, versionUrl, context),
  ])
  const indexed = packageDoc.versions?.find((item) => item.num === version)
  const exact = exactDoc.version
  requireFact(indexed && exact?.num === version, 'missing_exact_version', `crate:${entry.name}@${version} is absent from official metadata`)
  requireFact(indexed.checksum && indexed.checksum === exact.checksum, 'checksum_mismatch', `crate:${entry.name}@${version} checksum endpoints disagree`)
  requireFact(indexed.yanked === exact.yanked, 'yank_mismatch', `crate:${entry.name}@${version} yank endpoints disagree`)
  const expectedRepository = repositoryUrl(entry.repository)
  const packageRepository = repositoryUrl(packageDoc.crate?.repository)
  const exactRepository = repositoryUrl(exactDoc.crate?.repository ?? packageDoc.crate?.repository)
  requireFact(packageRepository === expectedRepository && exactRepository === expectedRepository, 'repository_mismatch', `crate:${entry.name}@${version} repository evidence is contradictory`)
  const publishedAt = isoDate(exact.created_at ?? indexed.created_at, `crate:${entry.name}@${version}`)
  const packageCreatedAt = isoDate(packageDoc.crate?.created_at, `crate:${entry.name}`)
  const downloads = packageDoc.crate?.recent_downloads
  requireFact(Number.isFinite(downloads), 'missing_download_snapshot', `crate:${entry.name}@${version} has no numeric download snapshot`)
  const repository = await verifyGithubRepository(context.fetchImpl, entry.repository, context)
  const reasons = []
  if (exact.yanked) reasons.push('exact release is yanked')
  if (ageDays(packageCreatedAt, context.now) < context.policy.minimumAgeDays) reasons.push(`package age below ${context.policy.minimumAgeDays} days`)
  if (downloads < context.policy.minimumWeeklyDownloads) reasons.push(`recent downloads below ${context.policy.minimumWeeklyDownloads}`)

  return {
    accepted: reasons.length === 0,
    reasons,
    evidence: {
      name: entry.name,
      version,
      repository: repository.repository,
      publishedAt: publishedAt.toISOString(),
      checksum: exact.checksum,
      yanked: exact.yanked,
      numericSnapshot: {
        metric: 'recentDownloads',
        value: downloads,
        source: packageUrl,
      },
      sources: { package: packageUrl, version: versionUrl, repository: repository.source },
      legitimacy: { verdict: reasons.length === 0 ? 'OK' : 'SUS', reasons },
      lockIntent: entry.lockIntent,
    },
  }
}

async function selectCandidate(kind, entry, context) {
  const lockIndex = entry.candidates.indexOf(entry.lockIntent.version)
  for (const [candidateIndex, version] of entry.candidates.entries()) {
    const inspected = kind === 'npm'
      ? await inspectNpmCandidate(entry, version, context)
      : await inspectCrateCandidate(entry, version, context)
    if (candidateIndex < lockIndex) continue
    requireFact(
      inspected.accepted,
      'legitimacy_not_ok',
      `${kind}:${entry.name}@${version} admitted lock intent is no longer OK: ${inspected.reasons.join('; ')}`,
    )
    return { ...inspected.evidence, candidateIndex, candidatesConsidered: candidateIndex + 1 }
  }
  throw new EvidenceError('stale_lock_intent', `${kind}:${entry.name} lock intent ${entry.lockIntent.version} was not evaluated`)
}

export async function buildPhase2DependencyReport(config, options = {}) {
  validateConfiguration(config)
  const now = options.now instanceof Date ? options.now : new Date(options.now ?? Date.now())
  requireFact(Number.isFinite(now.getTime()), 'invalid_clock', 'verification clock is invalid')
  const context = {
    fetchImpl: options.fetchImpl ?? globalThis.fetch,
    now,
    policy: config.phase2.policy,
    timeoutMs: options.timeoutMs ?? config.timeoutMs ?? 5000,
    retries: options.retries ?? config.retries ?? 2,
  }
  requireFact(typeof context.fetchImpl === 'function', 'missing_fetch', 'a fetch implementation is required')

  const npm = []
  for (const entry of config.phase2.npm) npm.push(await selectCandidate('npm', entry, context))
  const crates = []
  for (const entry of config.phase2.crates) crates.push(await selectCandidate('crates', entry, context))
  const lockIntent = {
    npm: config.phase2.npm.map(({ name, lockIntent: intent }) => ({ name, ...intent })),
    crates: config.phase2.crates.map(({ name, lockIntent: intent }) => ({ name, ...intent })),
  }
  return {
    schemaVersion: 1,
    status: 'success',
    phase: 'FLOWPDF-02',
    observedAt: now.toISOString(),
    policy: config.phase2.policy,
    lockIntentDigest: stableDigest(lockIntent),
    lockIntent,
    npm,
    crates,
  }
}

async function atomicJsonWrite(destination, value) {
  await mkdir(path.dirname(destination), { recursive: true })
  const temporary = `${destination}.${process.pid}.tmp`
  await writeFile(temporary, `${JSON.stringify(value, null, 2)}\n`, 'utf8')
  await rename(temporary, destination)
}

export async function verifyPhase2Dependencies(options = {}) {
  const configPath = options.configPath ?? DEFAULT_CONFIG
  const reportPath = options.reportPath ?? DEFAULT_REPORT
  const blockerPath = options.blockerPath ?? DEFAULT_BLOCKER
  try {
    const config = options.config ?? JSON.parse(await readFile(configPath, 'utf8'))
    const report = await buildPhase2DependencyReport(config, options)
    await atomicJsonWrite(reportPath, report)
    await rm(blockerPath, { force: true })
    return report
  } catch (error) {
    await rm(reportPath, { force: true })
    const blocker = {
      schemaVersion: 1,
      status: 'blocked',
      phase: 'FLOWPDF-02',
      observedAt: (options.now instanceof Date ? options.now : new Date(options.now ?? Date.now())).toISOString(),
      code: error.code ?? 'verification_failed',
      reason: error.message,
      overrideAllowed: false,
    }
    await atomicJsonWrite(blockerPath, blocker)
    throw error
  }
}

async function main() {
  const report = await verifyPhase2Dependencies()
  console.log(`Verified ${report.npm.length} npm packages and ${report.crates.length} crates; exact lock intent ${report.lockIntentDigest}.`)
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch((error) => {
    console.error(error.message)
    process.exitCode = 1
  })
}
