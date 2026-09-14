#!/usr/bin/env node

import { createHash } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'

export const projectRoot = resolve(import.meta.dirname, '..')
export const uiSpecPath = '.planning/phases/FLOWPDF-02-accessible-rich-text-editing/02-UI-SPEC.md'
export const contractPath = 'tests/contracts/phase2-ui-considerations.json'
export const phase = 'FLOWPDF-02-accessible-rich-text-editing'

const expectedCategories = Object.freeze([
  'empty',
  'loading',
  'error',
  'populated',
  'partial',
  'overflow',
  'zero-one-many',
  'long-text',
])

// This is the approved, explicit applicability matrix from the UI-SPEC taxonomy.
// It is intentionally closed: an omitted pair is a generator error, not a backstop.
const categorySurfaces = Object.freeze({
  empty: Object.freeze([
    'EditorAppShell',
    'FormattingToolbar',
    'BlockStyleSelect',
    'InlineMarkGroup',
    'InsertMenu',
    'DocumentEditorRegion',
    'ImageInsertDialog',
    'TableInsertDialog',
    'SimpleTableBlock',
    'StatusRegion',
    'ErrorRegion',
    'DestructiveConfirmDialog',
    'FontFamilySelect',
    'DiagnosticsEntry',
  ]),
  loading: Object.freeze([
    'EditorAppShell',
    'ApplicationBar',
    'FormattingToolbar',
    'InsertMenu',
    'DocumentEditorRegion',
    'ImageInsertDialog',
    'ImageBlock',
    'TableInsertDialog',
    'BlockActionBar',
    'StatusRegion',
    'ErrorRegion',
    'DestructiveConfirmDialog',
  ]),
  error: Object.freeze([
    'EditorAppShell',
    'FormattingToolbar',
    'BlockStyleSelect',
    'InlineMarkGroup',
    'InsertMenu',
    'DocumentEditorRegion',
    'ControlledInputProjection',
    'ImageInsertDialog',
    'ImageBlock',
    'TableInsertDialog',
    'SimpleTableBlock',
    'BlockActionBar',
    'StatusRegion',
    'ErrorRegion',
    'DestructiveConfirmDialog',
  ]),
  populated: Object.freeze([
    'ApplicationBar',
    'FormattingToolbar',
    'BlockStyleSelect',
    'InlineMarkGroup',
    'FontFamilySelect',
    'InsertMenu',
    'DocumentEditorRegion',
    'ControlledInputProjection',
    'BlockActionBar',
    'ImageBlock',
    'SimpleTableBlock',
    'TableActionBar',
    'PageBreakBlock',
    'StatusRegion',
    'DiagnosticsEntry',
  ]),
  partial: Object.freeze([
    'FormattingToolbar',
    'InlineMarkGroup',
    'FontFamilySelect',
    'ImageInsertDialog',
    'ImageBlock',
    'TableInsertDialog',
    'SimpleTableBlock',
    'StatusRegion',
    'ErrorRegion',
    'DocumentEditorRegion',
    'BlockStyleSelect',
    'TableActionBar',
  ]),
  overflow: Object.freeze([
    'EditorAppShell',
    'ApplicationBar',
    'FormattingToolbar',
    'InsertMenu',
    'DocumentEditorRegion',
    'ImageInsertDialog',
    'ImageBlock',
    'SimpleTableBlock',
    'TableActionBar',
    'PageBreakBlock',
    'StatusRegion',
    'ErrorRegion',
    'DestructiveConfirmDialog',
    'FontFamilySelect',
  ]),
  'zero-one-many': Object.freeze([
    'DocumentEditorRegion',
    'ControlledInputProjection',
    'InsertMenu',
    'ImageBlock',
    'SimpleTableBlock',
    'TableActionBar',
    'BlockActionBar',
    'StatusRegion',
    'FormattingToolbar',
    'ApplicationBar',
    'ImageInsertDialog',
    'TableInsertDialog',
    'DiagnosticsEntry',
  ]),
  'long-text': Object.freeze([
    'EditorAppShell',
    'ApplicationBar',
    'FormattingToolbar',
    'BlockStyleSelect',
    'InlineMarkGroup',
    'FontFamilySelect',
    'InsertMenu',
    'DocumentEditorRegion',
    'ControlledInputProjection',
    'ImageInsertDialog',
    'TableInsertDialog',
    'StatusRegion',
    'ErrorRegion',
  ]),
})

const categoryOwners = Object.freeze({
  empty: Object.freeze({
    kind: 'browser-assertion',
    file: 'web/tests/editor-ui-states.browser.test.ts',
    symbol: 'assertEmptyState',
  }),
  loading: Object.freeze({
    kind: 'browser-assertion',
    file: 'web/tests/editor-ui-states.browser.test.ts',
    symbol: 'assertLoadingState',
  }),
  error: Object.freeze({
    kind: 'browser-assertion',
    file: 'web/tests/editor-ui-states.browser.test.ts',
    symbol: 'assertErrorState',
  }),
  populated: Object.freeze({
    kind: 'browser-assertion',
    file: 'web/tests/editor-ui-states.browser.test.ts',
    symbol: 'assertPopulatedState',
  }),
  partial: Object.freeze({
    kind: 'browser-assertion',
    file: 'web/tests/editor-ui-states.browser.test.ts',
    symbol: 'assertPartialState',
  }),
  overflow: Object.freeze({
    kind: 'browser-assertion',
    file: 'web/tests/editor-responsive.browser.test.ts',
    symbol: 'assertResponsiveOverflow',
  }),
  'zero-one-many': Object.freeze({
    kind: 'browser-assertion',
    file: 'web/tests/editor-ui-states.browser.test.ts',
    symbol: 'assertZeroOneManyState',
  }),
  'long-text': Object.freeze({
    kind: 'browser-assertion',
    file: 'web/tests/editor-ui-states.browser.test.ts',
    symbol: 'assertLongTextState',
  }),
})

export function parseMarkdownTable(markdown, heading, nextHeadingPattern) {
  const start = markdown.indexOf(heading)
  if (start < 0) throw new Error(`missing approved UI-SPEC heading: ${heading}`)
  const afterHeading = markdown.slice(start + heading.length)
  const endMatch = afterHeading.match(nextHeadingPattern)
  const section = endMatch === null ? afterHeading : afterHeading.slice(0, endMatch.index)
  return section
    .split('\n')
    .filter((line) => /^\|.*\|$/.test(line.trim()))
    .map((line) => line.trim().slice(1, -1).split('|').map((cell) => cell.trim()))
    .filter((cells) => cells.length > 0 && !cells.every((cell) => /^-+$/.test(cell)))
    .slice(1)
}

export function parseApprovedUiSpec(markdown) {
  const inventoryRows = parseMarkdownTable(
    markdown,
    '## Component and Element Inventory',
    /\n## Interaction Contract/,
  )
  const taxonomyRows = parseMarkdownTable(
    markdown,
    '## UI Considerations',
    /\n## Registry Safety/,
  )
  const inventory = inventoryRows.map(([name, semanticContract, states]) => ({
    name,
    semanticContract,
    states,
  }))
  const taxonomy = taxonomyRows.map(([category, elements, status, resolution]) => ({
    category,
    elements,
    status,
    resolution,
  }))
  return { inventory, taxonomy }
}

function digest(value) {
  return `sha256:${createHash('sha256').update(value).digest('hex')}`
}

function lineNumber(markdown, needle) {
  const index = markdown.indexOf(needle)
  if (index < 0) return null
  return markdown.slice(0, index).split('\n').length
}

function kebabCase(value) {
  return value
    .replace(/([a-z0-9])([A-Z])/g, '$1-$2')
    .replace(/[^a-zA-Z0-9]+/g, '-')
    .toLowerCase()
}

function canonicalInventory(inventory) {
  return JSON.stringify(inventory)
}

function canonicalTaxonomy(taxonomy) {
  return JSON.stringify(taxonomy)
}

export function buildPhaseTwoUiContract({ root = projectRoot, sourceText } = {}) {
  const markdown = sourceText ?? readFileSync(resolve(root, uiSpecPath), 'utf8')
  const { inventory, taxonomy } = parseApprovedUiSpec(markdown)
  const inventoryNames = new Set(inventory.map(({ name }) => name))
  const taxonomyNames = taxonomy.map(({ category }) => category)

  if (JSON.stringify(taxonomyNames) !== JSON.stringify(expectedCategories)) {
    throw new Error(
      `UI-SPEC taxonomy drifted: expected ${expectedCategories.join(',')}, got ${taxonomyNames.join(',')}`,
    )
  }
  for (const category of expectedCategories) {
    if (!(category in categorySurfaces)) throw new Error(`missing category matrix: ${category}`)
    if (!(category in categoryOwners)) throw new Error(`missing category owner: ${category}`)
    for (const surface of categorySurfaces[category]) {
      if (!inventoryNames.has(surface)) {
        throw new Error(`surface ${surface} is not in the approved UI-SPEC inventory`)
      }
    }
  }

  const sourceDigest = digest(markdown)
  const inventoryDigest = digest(canonicalInventory(inventory))
  const taxonomyDigest = digest(canonicalTaxonomy(taxonomy))
  const entries = []
  for (const category of expectedCategories) {
    const owner = categoryOwners[category]
    const seenSurfaces = new Set()
    for (const surface of categorySurfaces[category]) {
      if (seenSurfaces.has(surface)) throw new Error(`duplicate explicit pair: ${category}:${surface}`)
      seenSurfaces.add(surface)
      entries.push({
        id: `${category}:${kebabCase(surface)}`,
        category,
        surface,
        classification: 'explicit',
        source: {
          section: 'UI Considerations',
          categoryLine: lineNumber(markdown, `| ${category} |`),
          surfaceLine: lineNumber(markdown, `| ${surface} |`),
        },
        owner,
      })
    }
  }

  const contract = {
    formatVersion: 1,
    phase,
    contract: 'UI-COVERAGE-108',
    source: {
      path: uiSpecPath,
      digest: sourceDigest,
      inventoryDigest,
      taxonomyDigest,
    },
    taxonomy: expectedCategories,
    counts: {
      sourceExplicit: entries.length,
      coveredExplicit: entries.length,
      backstop: 0,
      unresolved: 0,
      unclassified: 0,
    },
    backstop: [],
    unresolved: [],
    unclassified: [],
    entries,
  }
  validatePhaseTwoUiContract(contract, { root, sourceText: markdown })
  return contract
}

export function validatePhaseTwoUiContract(contract, { root = projectRoot, sourceText } = {}) {
  if (contract === null || typeof contract !== 'object') {
    throw new Error('UI contract must be an object')
  }
  const markdown = sourceText ?? readFileSync(resolve(root, uiSpecPath), 'utf8')
  const expected = buildExpectedMetadata(markdown)
  if (contract.formatVersion !== 1) throw new Error('UI contract formatVersion must be 1')
  if (contract.phase !== phase) throw new Error('UI contract phase is incorrect')
  if (contract.contract !== 'UI-COVERAGE-108') throw new Error('UI contract identity is incorrect')
  if (contract.source?.digest !== expected.sourceDigest) throw new Error('UI-SPEC source digest drifted')
  if (contract.source?.inventoryDigest !== expected.inventoryDigest) {
    throw new Error('UI-SPEC inventory digest drifted')
  }
  if (contract.source?.taxonomyDigest !== expected.taxonomyDigest) {
    throw new Error('UI-SPEC taxonomy digest drifted')
  }
  if (JSON.stringify(contract.taxonomy) !== JSON.stringify(expectedCategories)) {
    throw new Error('UI contract taxonomy is not the approved ordered taxonomy')
  }
  if (!Array.isArray(contract.entries)) throw new Error('UI contract entries must be an array')
  for (const bucket of ['backstop', 'unresolved', 'unclassified']) {
    if (!Array.isArray(contract[bucket]) || contract[bucket].length !== 0) {
      throw new Error(`UI contract ${bucket} bucket must be empty`)
    }
  }
  const expectedCount = Object.values(categorySurfaces).reduce((sum, surfaces) => sum + surfaces.length, 0)
  if (contract.entries.length !== expectedCount) {
    throw new Error(`UI contract entry count must be ${expectedCount}`)
  }
  const ids = new Set()
  const expectedPairs = new Set(
    expectedCategories.flatMap((category) =>
      categorySurfaces[category].map((surface) => `${category}:${kebabCase(surface)}`),
    ),
  )
  for (const entry of contract.entries) {
    if (entry.classification !== 'explicit') throw new Error(`non-explicit UI pair: ${entry.id}`)
    if (ids.has(entry.id)) throw new Error(`duplicate UI pair: ${entry.id}`)
    ids.add(entry.id)
    if (!expectedPairs.has(entry.id)) throw new Error(`stale or unapproved UI pair: ${entry.id}`)
    if (entry.source?.categoryLine === null || entry.source?.surfaceLine === null) {
      throw new Error(`UI pair has no source line: ${entry.id}`)
    }
    const owner = entry.owner
    if (
      owner?.kind !== 'browser-assertion' ||
      typeof owner.file !== 'string' ||
      typeof owner.symbol !== 'string'
    ) {
      throw new Error(`UI pair has no executable owner: ${entry.id}`)
    }
    const ownerText = readFileSync(resolve(root, owner.file), 'utf8')
    if (!ownerText.includes(owner.symbol)) {
      throw new Error(`UI pair owner is stale: ${entry.id} -> ${owner.file}#${owner.symbol}`)
    }
  }
  if (ids.size !== expectedPairs.size) throw new Error('UI contract has omitted explicit pairs')
  const counts = contract.counts
  const exactCounts = {
    sourceExplicit: expectedCount,
    coveredExplicit: expectedCount,
    backstop: 0,
    unresolved: 0,
    unclassified: 0,
  }
  if (JSON.stringify(counts) !== JSON.stringify(exactCounts)) {
    throw new Error(`UI contract counts must equal ${JSON.stringify(exactCounts)}`)
  }
  return true
}

function buildExpectedMetadata(markdown) {
  const { inventory, taxonomy } = parseApprovedUiSpec(markdown)
  return {
    sourceDigest: digest(markdown),
    inventoryDigest: digest(canonicalInventory(inventory)),
    taxonomyDigest: digest(canonicalTaxonomy(taxonomy)),
  }
}

export function renderPhaseTwoUiContract(contract) {
  return `${JSON.stringify(contract, null, 2)}\n`
}

function main() {
  const contract = buildPhaseTwoUiContract()
  const outputPath = resolve(projectRoot, contractPath)
  if (process.argv.includes('--check')) {
    const existing = JSON.parse(readFileSync(outputPath, 'utf8'))
    validatePhaseTwoUiContract(existing)
    const expectedText = renderPhaseTwoUiContract(contract)
    const actualText = readFileSync(outputPath, 'utf8')
    if (actualText !== expectedText) {
      throw new Error('checked-in Phase 2 UI contract is stale or not deterministically formatted')
    }
  } else {
    writeFileSync(outputPath, renderPhaseTwoUiContract(contract), 'utf8')
  }
  process.stdout.write(
    `UI-COVERAGE-108 sourceExplicit=${contract.counts.sourceExplicit} ` +
      `coveredExplicit=${contract.counts.coveredExplicit} backstop=${contract.counts.backstop} ` +
      `unresolved=${contract.counts.unresolved} unclassified=${contract.counts.unclassified}\n`,
  )
}

if (process.argv[1] !== undefined && resolve(process.argv[1]) === resolve(import.meta.url.slice(7))) {
  try {
    main()
  } catch (error) {
    process.stderr.write(`${error instanceof Error ? error.message : String(error)}\n`)
    process.exitCode = 1
  }
}
