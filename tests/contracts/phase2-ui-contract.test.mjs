import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import test from 'node:test'

import {
  buildPhaseTwoUiContract,
  renderPhaseTwoUiContract,
  validatePhaseTwoUiContract,
} from '../../scripts/build-phase2-ui-contract.mjs'

const projectRoot = resolve(import.meta.dirname, '../..')
const contractFile = resolve(projectRoot, 'tests/contracts/phase2-ui-considerations.json')

function loadContract() {
  return JSON.parse(readFileSync(contractFile, 'utf8'))
}

test('UI-COVERAGE-108 contains exactly 108 explicit executable owners', () => {
  const contract = loadContract()
  validatePhaseTwoUiContract(contract, { root: projectRoot })
  assert.deepEqual(contract.counts, {
    sourceExplicit: 108,
    coveredExplicit: 108,
    backstop: 0,
    unresolved: 0,
    unclassified: 0,
  })
  assert.equal(contract.entries.length, 108)
  assert.equal(new Set(contract.entries.map((entry) => entry.id)).size, 108)
})

test('UI-COVERAGE-108 generation is byte-identical and source-digested', () => {
  const first = buildPhaseTwoUiContract({ root: projectRoot })
  const second = buildPhaseTwoUiContract({ root: projectRoot })
  assert.equal(renderPhaseTwoUiContract(first), renderPhaseTwoUiContract(second))
  assert.equal(renderPhaseTwoUiContract(first), readFileSync(contractFile, 'utf8'))
  assert.match(first.source.digest, /^sha256:[0-9a-f]{64}$/)
  assert.match(first.source.inventoryDigest, /^sha256:[0-9a-f]{64}$/)
  assert.match(first.source.taxonomyDigest, /^sha256:[0-9a-f]{64}$/)
})

test('UI contract fails closed for an omitted explicit pair', () => {
  const contract = loadContract()
  contract.entries = contract.entries.slice(1)
  assert.throws(
    () => validatePhaseTwoUiContract(contract, { root: projectRoot }),
    /entry count must be 108|omitted explicit pairs/,
  )
})

test('UI contract fails closed for a duplicate explicit pair', () => {
  const contract = loadContract()
  contract.entries.push({ ...contract.entries[0] })
  assert.throws(
    () => validatePhaseTwoUiContract(contract, { root: projectRoot }),
    /entry count must be 108|duplicate UI pair/,
  )
})

test('UI contract fails closed for source drift', () => {
  const contract = loadContract()
  contract.source.digest = 'sha256:' + '0'.repeat(64)
  assert.throws(
    () => validatePhaseTwoUiContract(contract, { root: projectRoot }),
    /source digest drifted/,
  )
})

test('UI contract fails closed for backstop or unresolved fallback classification', () => {
  const contract = loadContract()
  contract.entries[0].classification = 'backstop'
  assert.throws(
    () => validatePhaseTwoUiContract(contract, { root: projectRoot }),
    /non-explicit UI pair/,
  )

  const unresolved = loadContract()
  unresolved.unresolved = [{ id: 'empty:editor-app-shell' }]
  assert.throws(
    () => validatePhaseTwoUiContract(unresolved, { root: projectRoot }),
    /unresolved bucket must be empty/,
  )
})

test('UI contract fails closed when an executable owner goes stale', () => {
  const contract = loadContract()
  contract.entries[0].owner.symbol = 'assertMissingOwner'
  assert.throws(
    () => validatePhaseTwoUiContract(contract, { root: projectRoot }),
    /owner is stale/,
  )
})
