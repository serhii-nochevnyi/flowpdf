import assert from 'node:assert/strict'
import { copyFile, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises'
import os from 'node:os'
import path from 'node:path'
import test from 'node:test'
import { fileURLToPath } from 'node:url'

import { verifyUnicodeCorpus } from './verify-unicode-corpus.mjs'

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const CORPUS = path.join(ROOT, 'fixtures/unicode/17.0.0/GraphemeBreakTest.txt')
const PROVENANCE = path.join(ROOT, 'fixtures/unicode/17.0.0/provenance.json')
const DEPENDENCY_REPORT = path.join(ROOT, 'artifacts/provenance/phase2-dependencies.json')

async function withFixture(run) {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'flowpdf-unicode-corpus-'))
  const corpusPath = path.join(directory, 'GraphemeBreakTest.txt')
  const provenancePath = path.join(directory, 'provenance.json')
  const dependencyReportPath = path.join(directory, 'phase2-dependencies.json')
  await Promise.all([
    copyFile(CORPUS, corpusPath),
    copyFile(PROVENANCE, provenancePath),
    copyFile(DEPENDENCY_REPORT, dependencyReportPath),
  ])
  try {
    return await run({ corpusPath, provenancePath, dependencyReportPath })
  } finally {
    await rm(directory, { recursive: true, force: true })
  }
}

async function mutateJson(file, mutate) {
  const value = JSON.parse(await readFile(file, 'utf8'))
  mutate(value)
  await writeFile(file, `${JSON.stringify(value, null, 2)}\n`, 'utf8')
}

async function rejectsCode(paths, code) {
  await assert.rejects(
    verifyUnicodeCorpus(paths),
    (error) => error.code === code,
  )
}

test('verifies the pinned corpus deterministically and offline', async () => {
  await withFixture(async (paths) => {
    const first = await verifyUnicodeCorpus(paths)
    const second = await verifyUnicodeCorpus(paths)
    assert.deepEqual(first, second)
    assert.equal(first.status, 'verified')
    assert.equal(first.unicodeVersion, '17.0.0')
    assert.equal(first.byteLength, 126_570)
    assert.equal(first.sha256, 'e2d134d2c52919bace503ebb6a551c1855fe1a1faec18478c78fff254a1793ec')
    assert.equal(first.icu4x.crateVersion, '2.3.0')
  })
})

test('rejects altered corpus bytes even when the byte count is unchanged', async () => {
  await withFixture(async (paths) => {
    const bytes = await readFile(paths.corpusPath)
    bytes[bytes.length - 2] ^= 1
    await writeFile(paths.corpusPath, bytes)
    await rejectsCode(paths, 'corpus_hash_mismatch')
  })
})

test('rejects independently altered corpus identity metadata', async () => {
  const cases = [
    ['sourceUrl', (value) => { value.corpus.sourceUrl = 'https://example.invalid/GraphemeBreakTest.txt' }],
    ['byteLength', (value) => { value.corpus.byteLength += 1 }],
    ['sha256', (value) => { value.corpus.sha256 = '0'.repeat(64) }],
    ['unicodeVersion', (value) => { value.corpus.unicodeVersion = '16.0.0' }],
    ['header', (value) => { value.corpus.header = '# GraphemeBreakTest-16.0.0.txt' }],
  ]
  for (const [name, mutate] of cases) {
    await withFixture(async (paths) => {
      await mutateJson(paths.provenancePath, mutate)
      await assert.rejects(
        verifyUnicodeCorpus(paths),
        (error) => error.code === 'provenance_mismatch' && error.message.includes(name),
      )
    })
  }
})

test('rejects independently altered ICU4X crate and data tags', async () => {
  const cases = [
    ['crateVersion', '2.2.0'],
    ['testedUnicodeTag', '16.0.0'],
    ['testedCldrTag', '47.0.0'],
    ['testedIcuExportTag', 'release-77-1'],
    ['testedSegmenterLstmTag', 'v9.9.9'],
  ]
  for (const [field, replacement] of cases) {
    await withFixture(async (paths) => {
      await mutateJson(paths.provenancePath, (value) => { value.icu4x[field] = replacement })
      await assert.rejects(
        verifyUnicodeCorpus(paths),
        (error) => error.code === 'provenance_mismatch' && error.message.includes(field),
      )
    })
  }
})

test('rejects drift from the accepted ICU dependency identity and closed feature set', async () => {
  const cases = [
    ['version', (entry) => { entry.version = '2.2.0' }],
    ['repository', (entry) => { entry.repository = 'https://github.com/example/impostor' }],
    ['features', (entry) => { entry.lockIntent.features = ['compiled_data', 'auto'] }],
    ['legitimacy', (entry) => { entry.legitimacy.verdict = 'SUS' }],
  ]
  for (const [name, mutate] of cases) {
    await withFixture(async (paths) => {
      await mutateJson(paths.dependencyReportPath, (value) => {
        mutate(value.crates.find((entry) => entry.name === 'icu_segmenter'))
      })
      await assert.rejects(
        verifyUnicodeCorpus(paths),
        (error) => error.code === 'dependency_provenance_mismatch' && error.message.includes(name),
      )
    })
  }
})

test('rejects an oversized provenance envelope before JSON parsing', async () => {
  await withFixture(async (paths) => {
    await writeFile(paths.provenancePath, '{'.padEnd(20_000, 'x'), 'utf8')
    await rejectsCode(paths, 'provenance_too_large')
  })
})

test('drift diagnostics expose stable facts without leaking altered corpus bytes', async () => {
  await withFixture(async (paths) => {
    const bytes = await readFile(paths.corpusPath)
    const canary = Buffer.from('FLOWPDF_PRIVATE_CORPUS_CANARY')
    canary.copy(bytes, 100)
    await writeFile(paths.corpusPath, bytes)
    await assert.rejects(
      verifyUnicodeCorpus(paths),
      (error) => error.code === 'corpus_hash_mismatch' && !error.message.includes(canary.toString()),
    )
  })
})
