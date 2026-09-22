import assert from 'node:assert/strict'
import test from 'node:test'

import { assertCleanTree } from './verify-release-tree.mjs'

test('release tree accepts only an empty porcelain status', () => {
  assert.doesNotThrow(() => assertCleanTree(''))
  assert.throws(() => assertCleanTree(' M src/file.ts'), /clean tracked and untracked/)
  assert.throws(() => assertCleanTree('?? .planning/state.json'), /clean tracked and untracked/)
})
