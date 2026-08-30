#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { readFile, stat } from 'node:fs/promises'
import path from 'node:path'
import { pathToFileURL } from 'node:url'

import { assertSupportedNodeVersion } from './node-version.mjs'

assertSupportedNodeVersion()

const ROOT = path.resolve(import.meta.dirname, '..')
const DEFAULT_CORPUS = path.resolve(ROOT, 'fixtures/unicode/17.0.0/GraphemeBreakTest.txt')
const DEFAULT_PROVENANCE = path.resolve(ROOT, 'fixtures/unicode/17.0.0/provenance.json')
const DEFAULT_DEPENDENCY_REPORT = path.resolve(ROOT, 'artifacts/provenance/phase2-dependencies.json')
const MAX_PROVENANCE_BYTES = 16 * 1024
const MAX_DEPENDENCY_REPORT_BYTES = 512 * 1024

export const EXPECTED_UNICODE_CORPUS = Object.freeze({
  schemaVersion: 1,
  corpus: Object.freeze({
    relativePath: 'fixtures/unicode/17.0.0/GraphemeBreakTest.txt',
    sourceUrl: 'https://www.unicode.org/Public/17.0.0/ucd/auxiliary/GraphemeBreakTest.txt',
    byteLength: 126_570,
    sha256: 'e2d134d2c52919bace503ebb6a551c1855fe1a1faec18478c78fff254a1793ec',
    unicodeVersion: '17.0.0',
    header: '# GraphemeBreakTest-17.0.0.txt',
    retrievedAt: '2026-08-30',
    licenseUrl: 'https://www.unicode.org/terms_of_use.html',
  }),
  icu4x: Object.freeze({
    crate: 'icu_segmenter',
    crateVersion: '2.3.0',
    dataCrate: 'icu_segmenter_data',
    dataCrateVersion: '2.3.0',
    repository: 'https://github.com/unicode-org/icu4x',
    testedUnicodeTag: '17.0.0',
    testedCldrTag: '48.2.1',
    testedIcuExportTag: 'release-78.1rc',
    testedSegmenterLstmTag: 'v0.1.0',
    features: Object.freeze(['compiled_data']),
    crateSourceUrl: 'https://docs.rs/icu_segmenter/2.3.0/src/icu_segmenter/grapheme.rs.html',
    dataSourceUrl: 'https://docs.rs/crate/icu_segmenter_data/2.3.0/source/Cargo.toml.orig',
  }),
  dependencyEvidence: Object.freeze({
    relativePath: 'artifacts/provenance/phase2-dependencies.json',
    phase: 'FLOWPDF-02',
    status: 'success',
    lockIntentDigest: '27c9a5f2f41d0148ee1ff8e277ee1990f48135f367f555d709daed7604666bae',
    crateChecksum: '82d07aafccd67af15d02512a6adf5896fbc5ed00f2e99b471d2efa14016db3db',
  }),
})

export class UnicodeCorpusError extends Error {
  constructor(code, message) {
    super(message)
    this.name = 'UnicodeCorpusError'
    this.code = code
  }
}

function fail(code, message) {
  throw new UnicodeCorpusError(code, message)
}

async function readBoundedFile(file, maximumBytes, tooLargeCode, label) {
  let details
  try {
    details = await stat(file)
  } catch {
    fail('missing_evidence', `${label} is unavailable`)
  }
  if (!details.isFile()) fail('invalid_evidence_type', `${label} must be a regular file`)
  if (details.size > maximumBytes) fail(tooLargeCode, `${label} exceeds its verification byte limit`)
  const bytes = await readFile(file)
  if (bytes.byteLength > maximumBytes) {
    fail(tooLargeCode, `${label} exceeds its verification byte limit`)
  }
  return bytes
}

async function readBoundedJson(file, maximumBytes, tooLargeCode, label) {
  const bytes = await readBoundedFile(file, maximumBytes, tooLargeCode, label)
  try {
    return JSON.parse(new TextDecoder('utf-8', { fatal: true }).decode(bytes))
  } catch {
    fail('invalid_json', `${label} is not valid bounded UTF-8 JSON`)
  }
}

function requireObject(value, code, label) {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) {
    fail(code, `${label} must be an object`)
  }
}

function requireExactKeys(actual, expected, code, label) {
  requireObject(actual, code, label)
  const actualKeys = Object.keys(actual).sort()
  const expectedKeys = Object.keys(expected).sort()
  if (JSON.stringify(actualKeys) !== JSON.stringify(expectedKeys)) {
    fail(code, `${label} fields do not match the pinned schema`)
  }
}

function requirePinnedSection(actual, expected, section) {
  requireExactKeys(actual, expected, 'provenance_mismatch', `Unicode provenance ${section}`)
  for (const [field, expectedValue] of Object.entries(expected)) {
    const actualValue = actual[field]
    const matches = Array.isArray(expectedValue)
      ? Array.isArray(actualValue) && JSON.stringify(actualValue) === JSON.stringify(expectedValue)
      : actualValue === expectedValue
    if (!matches) {
      fail('provenance_mismatch', `Unicode provenance ${section}.${field} does not match the pinned value`)
    }
  }
}

function verifyProvenance(provenance) {
  requireExactKeys(provenance, EXPECTED_UNICODE_CORPUS, 'provenance_mismatch', 'Unicode provenance')
  if (provenance.schemaVersion !== EXPECTED_UNICODE_CORPUS.schemaVersion) {
    fail('provenance_mismatch', 'Unicode provenance schemaVersion does not match the pinned value')
  }
  requirePinnedSection(provenance.corpus, EXPECTED_UNICODE_CORPUS.corpus, 'corpus')
  requirePinnedSection(provenance.icu4x, EXPECTED_UNICODE_CORPUS.icu4x, 'icu4x')
  requirePinnedSection(
    provenance.dependencyEvidence,
    EXPECTED_UNICODE_CORPUS.dependencyEvidence,
    'dependencyEvidence',
  )
}

function dependencyFailure(field) {
  fail(
    'dependency_provenance_mismatch',
    `Accepted ICU dependency provenance ${field} does not match the pinned corpus authority`,
  )
}

function verifyDependencyEvidence(report) {
  requireObject(report, 'dependency_provenance_mismatch', 'Phase 2 dependency report')
  const expected = EXPECTED_UNICODE_CORPUS.dependencyEvidence
  if (report.phase !== expected.phase) dependencyFailure('phase')
  if (report.status !== expected.status) dependencyFailure('status')
  if (report.lockIntentDigest !== expected.lockIntentDigest) dependencyFailure('lockIntentDigest')
  if (!Array.isArray(report.crates)) dependencyFailure('crates')
  const matches = report.crates.filter((entry) => entry?.name === EXPECTED_UNICODE_CORPUS.icu4x.crate)
  if (matches.length !== 1) dependencyFailure('crate')
  const entry = matches[0]
  if (entry.version !== EXPECTED_UNICODE_CORPUS.icu4x.crateVersion) dependencyFailure('version')
  if (entry.repository !== EXPECTED_UNICODE_CORPUS.icu4x.repository) dependencyFailure('repository')
  if (entry.checksum !== expected.crateChecksum) dependencyFailure('checksum')
  if (entry.yanked !== false) dependencyFailure('yanked')
  if (entry.legitimacy?.verdict !== 'OK') dependencyFailure('legitimacy')
  if (entry.lockIntent?.version !== EXPECTED_UNICODE_CORPUS.icu4x.crateVersion) {
    dependencyFailure('lockIntent.version')
  }
  if (entry.lockIntent?.defaultFeatures !== false) dependencyFailure('defaultFeatures')
  if (
    !Array.isArray(entry.lockIntent?.features) ||
    JSON.stringify(entry.lockIntent.features) !== JSON.stringify(EXPECTED_UNICODE_CORPUS.icu4x.features)
  ) {
    dependencyFailure('features')
  }
}

async function verifyCorpusBytes(corpusPath) {
  let details
  try {
    details = await stat(corpusPath)
  } catch {
    fail('missing_evidence', 'Unicode grapheme corpus is unavailable')
  }
  if (!details.isFile()) fail('invalid_evidence_type', 'Unicode grapheme corpus must be a regular file')
  if (details.size !== EXPECTED_UNICODE_CORPUS.corpus.byteLength) {
    fail('corpus_size_mismatch', 'Unicode grapheme corpus byte count does not match the pinned release')
  }
  const bytes = await readFile(corpusPath)
  if (bytes.byteLength !== EXPECTED_UNICODE_CORPUS.corpus.byteLength) {
    fail('corpus_size_mismatch', 'Unicode grapheme corpus byte count does not match the pinned release')
  }
  const sha256 = createHash('sha256').update(bytes).digest('hex')
  if (sha256 !== EXPECTED_UNICODE_CORPUS.corpus.sha256) {
    fail('corpus_hash_mismatch', 'Unicode grapheme corpus SHA-256 does not match the pinned release')
  }
  let text
  try {
    text = new TextDecoder('utf-8', { fatal: true }).decode(bytes)
  } catch {
    fail('corpus_encoding_mismatch', 'Unicode grapheme corpus is not valid UTF-8')
  }
  if (text.split('\n', 1)[0] !== EXPECTED_UNICODE_CORPUS.corpus.header) {
    fail('corpus_version_mismatch', 'Unicode grapheme corpus header does not identify release 17.0.0')
  }
  return { byteLength: bytes.byteLength, sha256 }
}

export async function verifyUnicodeCorpus(options = {}) {
  const corpusPath = options.corpusPath ?? DEFAULT_CORPUS
  const provenancePath = options.provenancePath ?? DEFAULT_PROVENANCE
  const dependencyReportPath = options.dependencyReportPath ?? DEFAULT_DEPENDENCY_REPORT
  const [provenance, dependencyReport] = await Promise.all([
    readBoundedJson(
      provenancePath,
      MAX_PROVENANCE_BYTES,
      'provenance_too_large',
      'Unicode provenance',
    ),
    readBoundedJson(
      dependencyReportPath,
      MAX_DEPENDENCY_REPORT_BYTES,
      'dependency_provenance_too_large',
      'Phase 2 dependency report',
    ),
  ])
  verifyProvenance(provenance)
  verifyDependencyEvidence(dependencyReport)
  const corpus = await verifyCorpusBytes(corpusPath)
  return {
    status: 'verified',
    unicodeVersion: EXPECTED_UNICODE_CORPUS.corpus.unicodeVersion,
    sourceUrl: EXPECTED_UNICODE_CORPUS.corpus.sourceUrl,
    byteLength: corpus.byteLength,
    sha256: corpus.sha256,
    icu4x: {
      crateVersion: EXPECTED_UNICODE_CORPUS.icu4x.crateVersion,
      testedUnicodeTag: EXPECTED_UNICODE_CORPUS.icu4x.testedUnicodeTag,
      testedCldrTag: EXPECTED_UNICODE_CORPUS.icu4x.testedCldrTag,
      testedIcuExportTag: EXPECTED_UNICODE_CORPUS.icu4x.testedIcuExportTag,
      testedSegmenterLstmTag: EXPECTED_UNICODE_CORPUS.icu4x.testedSegmenterLstmTag,
    },
    dependencyLockIntentDigest: EXPECTED_UNICODE_CORPUS.dependencyEvidence.lockIntentDigest,
  }
}

async function main() {
  const result = await verifyUnicodeCorpus()
  process.stdout.write(
    `Verified Unicode ${result.unicodeVersion} GraphemeBreakTest: ${result.byteLength} bytes, sha256:${result.sha256}.\n`,
  )
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  main().catch((error) => {
    process.stderr.write(`${error.code ?? 'verification_failed'}: ${error.message}\n`)
    process.exitCode = 1
  })
}
