import assert from 'node:assert/strict'
import test from 'node:test'

import {
  loadScaleRecipes,
  projectRoot,
  runPhaseTwoScaleSmoke,
  runSemanticRecovery,
  validateScaleRecipe,
} from '../../scripts/verify-phase2-scale.mjs'

test('Phase 2 scale recipes are bounded semantic workloads with stable recovery', () => {
  const recipes = loadScaleRecipes(projectRoot)
  assert.deepEqual(
    recipes.map(({ recipe }) => recipe.name),
    ['phase2-semantic-scale-100', 'phase2-semantic-scale-200'],
  )
  for (const { recipe } of recipes) {
    const validation = validateScaleRecipe(recipe)
    const recovery = runSemanticRecovery(recipe)
    assert.equal(validation.semanticPayloadHash, recovery.semanticPayloadHash)
    assert.equal(recovery.recoveryHash.startsWith('sha256:'), true)
    assert.equal(recovery.revision, recipe.expected.finalRevision)
  }
})

test('Phase 2 scale smoke is deterministic and privacy-safe', () => {
  const first = runPhaseTwoScaleSmoke(projectRoot)
  const second = runPhaseTwoScaleSmoke(projectRoot)
  const withoutTiming = ({ elapsedMilliseconds: _ignored, ...diagnostic }) => diagnostic
  assert.deepEqual(withoutTiming(first), withoutTiming(second))
  assert.equal(first.status, 'pass')
  assert.equal(first.totalPages, 300)
  assert.equal(first.totalSemanticBlocks, 1542)
  const output = JSON.stringify(first)
  for (const forbidden of ['Український', 'English contract', '👩‍💻', 'апостроф']) {
    assert.equal(output.includes(forbidden), false, `diagnostics leaked ${forbidden}`)
  }
})

test('scale validation rejects malformed counts, stale hashes, and over-budget workloads', () => {
  const recipe = structuredClone(loadScaleRecipes(projectRoot)[0].recipe)

  recipe.pages = 101
  assert.throws(() => validateScaleRecipe(recipe), /counts do not match/)

  const stale = structuredClone(loadScaleRecipes(projectRoot)[0].recipe)
  stale.semanticPayloadHash = `sha256:${'0'.repeat(64)}`
  assert.throws(() => validateScaleRecipe(stale), /semantic hash is stale/)

  const overBudget = structuredClone(loadScaleRecipes(projectRoot)[0].recipe)
  overBudget.resourceBudget.maxSemanticBlocks = 1
  assert.throws(() => validateScaleRecipe(overBudget), /resource profile/)
})

test('scale validation rejects deferred geometry/PDF fields and forbidden authoring', () => {
  const deferred = structuredClone(loadScaleRecipes(projectRoot)[0].recipe)
  deferred.layout.pageWidth = 816
  assert.throws(() => validateScaleRecipe(deferred), /deferred layout or PDF behavior/)

  const authoring = structuredClone(loadScaleRecipes(projectRoot)[0].recipe)
  authoring.fields.authoring = true
  assert.throws(() => validateScaleRecipe(authoring), /field projection or deferred authoring/)
})
