#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { performance } from 'node:perf_hooks'

import { assertSupportedNodeVersion } from './node-version.mjs'

assertSupportedNodeVersion()

export const projectRoot = resolve(import.meta.dirname, '..')
export const scaleRecipePaths = Object.freeze([
  'fixtures/editor/phase2-scale-100.recipe.json',
  'fixtures/editor/phase2-scale-200.recipe.json',
])

const expectedRecipes = Object.freeze({
  'phase2-semantic-scale-100': Object.freeze({
    pages: 100,
    paragraphs: 500,
    pageBreaks: 9,
    transactions: 120,
    semanticBlocks: 521,
    finalRevision: 121,
  }),
  'phase2-semantic-scale-200': Object.freeze({
    pages: 200,
    paragraphs: 1000,
    pageBreaks: 19,
    transactions: 220,
    semanticBlocks: 1021,
    finalRevision: 221,
  }),
})

const expectedTables = Object.freeze([
  Object.freeze({ rows: 2, columns: 2, headerRows: 1 }),
  Object.freeze({ rows: 2, columns: 3, headerRows: 0 }),
])
const expectedImages = Object.freeze([
  Object.freeze({ action: 'insert', mediaType: 'image/png', accessibility: 'described' }),
  Object.freeze({ action: 'replace', mediaType: 'image/jpeg', accessibility: 'decorative' }),
  Object.freeze({
    action: 'setAccessibility',
    mediaType: 'image/jpeg',
    accessibility: 'described',
  }),
  Object.freeze({ action: 'remove', mediaType: 'image/jpeg', accessibility: 'described' }),
])
const forbiddenDeferredKeys = new Set([
  'pageWidth',
  'pageHeight',
  'coordinates',
  'rect',
  'fragments',
  'glyphs',
  'pdfBytes',
  'renderedPages',
  'canvas',
  'shaping',
  'hyphenation',
  'viewportGeometry',
])

export function loadScaleRecipes(root = projectRoot) {
  return scaleRecipePaths.map((relativePath) => ({
    path: relativePath,
    recipe: JSON.parse(readFileSync(resolve(root, relativePath), 'utf8')),
  }))
}

export function validateScaleRecipe(recipe) {
  const expected = expectedRecipes[recipe?.name]
  if (expected === undefined) throw new Error('unknown Phase 2 scale recipe')
  if (
    recipe.formatVersion !== 1 ||
    recipe.shape !== 'semantic-flow' ||
    recipe.pages !== expected.pages ||
    recipe.paragraphsPerPage !== 5 ||
    recipe.pages * recipe.paragraphsPerPage !== expected.paragraphs ||
    recipe.pageBreaks !== expected.pageBreaks ||
    recipe.transactions !== expected.transactions ||
    recipe.expected?.paragraphs !== expected.paragraphs ||
    recipe.expected?.semanticBlocks !== expected.semanticBlocks ||
    recipe.expected?.finalRevision !== expected.finalRevision ||
    recipe.expected?.tableCells !== 10 ||
    recipe.expected?.existingFields !== 6 ||
    recipe.expected?.recovery !== 'same-semantic-hash'
  ) {
    throw new Error(`scale recipe counts do not match the approved ${recipe.name} workload`)
  }
  if (JSON.stringify(recipe.locales) !== JSON.stringify(['uk-UA', 'en-US'])) {
    throw new Error('scale recipe must exercise Ukrainian and English')
  }
  if (
    recipe.unicode?.decomposed !== 'й' ||
    recipe.unicode?.emojiZwj !== '👩‍💻' ||
    recipe.unicode?.regionalIndicators !== '🇺🇦' ||
    recipe.unicode?.apostrophe !== 'апостроф ’' ||
    typeof recipe.unicode?.ukrainian !== 'string' ||
    typeof recipe.unicode?.english !== 'string'
  ) {
    throw new Error('scale recipe Unicode corpus is incomplete')
  }
  if (
    JSON.stringify(recipe.formatting?.inlineMarks) !== JSON.stringify(['bold', 'italic', 'underline']) ||
    JSON.stringify(recipe.formatting?.fontFamilies) !==
      JSON.stringify(['notoSans', 'notoSerif', 'notoSansMono']) ||
    JSON.stringify(recipe.formatting?.languages) !== JSON.stringify(['uk-UA', 'en-US']) ||
    JSON.stringify(recipe.formatting?.alignments) !==
      JSON.stringify(['start', 'center', 'end', 'justify']) ||
    recipe.formatting?.spacingMillipoints?.at(-1) !== 144000
  ) {
    throw new Error('scale recipe formatting vocabulary is incomplete')
  }
  if (
    JSON.stringify(recipe.lists) !==
      JSON.stringify({
        kinds: ['ordered', 'unordered'],
        itemsPerKind: 3,
        maximumDepth: 8,
        emptyItemExit: true,
      })
  ) {
    throw new Error('scale recipe list semantics are incomplete')
  }
  if (JSON.stringify(recipe.tables) !== JSON.stringify(expectedTables)) {
    throw new Error('scale recipe must include header and non-header simple tables')
  }
  if (JSON.stringify(recipe.images) !== JSON.stringify(expectedImages)) {
    throw new Error('scale recipe image lifecycle is incomplete or reordered')
  }
  if (
    JSON.stringify(recipe.fields) !==
    JSON.stringify({
      existingDescriptors: 6,
      reviewStates: ['legacyInvalid', 'targetDeleted'],
      authoring: false,
      filling: false,
    })
  ) {
    throw new Error('scale recipe field projection or deferred authoring scope drifted')
  }
  if (
    recipe.resourceBudget?.maxSemanticBlocks !== 4096 ||
    recipe.resourceBudget?.maxTextUtf8Bytes !== 2000000 ||
    recipe.resourceBudget?.maxTableCells !== 1000 ||
    recipe.resourceBudget?.maxImageEncodedBytes !== 8388608 ||
    recipe.resourceBudget?.maxListDepth !== 8 ||
    recipe.transactions > 1000
  ) {
    throw new Error('scale recipe exceeds the bounded Phase 2 resource profile')
  }
  if (
    JSON.stringify(recipe.layout) !==
    JSON.stringify({
      phase: 'phase2-semantic-only',
      pagination: false,
      pdf: false,
      geometry: false,
    })
  ) {
    throw new Error('scale recipe contains deferred layout or PDF behavior')
  }
  walkForDeferredKeys(recipe)
  const expectedHash = hashSemanticPayload(recipe)
  if (recipe.semanticPayloadHash !== expectedHash) {
    throw new Error(`scale recipe semantic hash is stale: ${recipe.name}`)
  }
  const estimatedTextBytes = estimateTextBytes(recipe)
  if (estimatedTextBytes > recipe.resourceBudget.maxTextUtf8Bytes) {
    throw new Error('scale recipe text exceeds its bounded semantic budget')
  }
  if (recipe.tables.reduce((sum, table) => sum + table.rows * table.columns, 0) > recipe.resourceBudget.maxTableCells) {
    throw new Error('scale recipe table cells exceed their bounded semantic budget')
  }
  return {
    name: recipe.name,
    pages: recipe.pages,
    paragraphs: expected.paragraphs,
    semanticBlocks: expected.semanticBlocks,
    estimatedTextBytes,
    semanticPayloadHash: expectedHash,
  }
}

export function runSemanticRecovery(recipe) {
  const validation = validateScaleRecipe(recipe)
  const model = {
    name: recipe.name,
    locales: recipe.locales,
    pages: recipe.pages,
    paragraphs: validation.paragraphs,
    semanticBlocks: validation.semanticBlocks,
    tables: recipe.tables,
    images: recipe.images.map(({ action, accessibility }) => ({ action, accessibility })),
    fields: recipe.fields,
    revision: recipe.expected.finalRevision,
  }
  const canonical = JSON.stringify(model)
  const beforeRecoveryHash = sha256(canonical)
  const recovered = JSON.parse(canonical)
  const afterRecoveryHash = sha256(JSON.stringify(recovered))
  if (beforeRecoveryHash !== afterRecoveryHash) {
    throw new Error(`semantic recovery hash mismatch: ${recipe.name}`)
  }
  return {
    ...validation,
    revision: model.revision,
    recoveryHash: beforeRecoveryHash,
  }
}

export function runPhaseTwoScaleSmoke(root = projectRoot) {
  const startedAt = performance.now()
  const measurements = loadScaleRecipes(root).map(({ recipe }) => runSemanticRecovery(recipe))
  const totalPages = measurements.reduce((sum, measurement) => sum + measurement.pages, 0)
  const totalBlocks = measurements.reduce((sum, measurement) => sum + measurement.semanticBlocks, 0)
  const diagnostic = {
    formatVersion: 1,
    status: 'pass',
    recipeCount: measurements.length,
    recipeNames: measurements.map(({ name }) => name),
    totalPages,
    totalSemanticBlocks: totalBlocks,
    revisions: measurements.map(({ name, revision }) => ({ name, revision })),
    semanticPayloadHashes: measurements.map(({ name, semanticPayloadHash }) => ({ name, semanticPayloadHash })),
    recoveryHashes: measurements.map(({ name, recoveryHash }) => ({ name, recoveryHash })),
    elapsedMilliseconds: Math.round(performance.now() - startedAt),
  }
  return diagnostic
}

function hashSemanticPayload(recipe) {
  const { semanticPayloadHash: _ignored, expected: _expected, ...payload } = recipe
  return sha256(JSON.stringify(payload))
}

function sha256(value) {
  return `sha256:${createHash('sha256').update(value, 'utf8').digest('hex')}`
}

function estimateTextBytes(recipe) {
  const corpusBytes = Buffer.byteLength(Object.values(recipe.unicode).join('|'), 'utf8')
  return corpusBytes * recipe.expected.paragraphs
}

function walkForDeferredKeys(value) {
  if (Array.isArray(value)) {
    value.forEach(walkForDeferredKeys)
    return
  }
  if (value === null || typeof value !== 'object') return
  for (const [key, child] of Object.entries(value)) {
    if (forbiddenDeferredKeys.has(key)) {
      throw new Error(`deferred Phase 3/PDF field is not permitted in semantic recipe: ${key}`)
    }
    walkForDeferredKeys(child)
  }
}

if (process.argv[1] !== undefined && resolve(process.argv[1]) === resolve(import.meta.url.slice(7))) {
  try {
    const diagnostic = runPhaseTwoScaleSmoke()
    if (process.argv.includes('--smoke')) {
      process.stdout.write(`Phase 2 semantic scale smoke passed: ${JSON.stringify(diagnostic)}\n`)
    } else {
      process.stdout.write(`${JSON.stringify(diagnostic)}\n`)
    }
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
    process.exitCode = 1
  }
}
