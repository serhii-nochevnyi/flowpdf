import assert from 'node:assert/strict'
import test from 'node:test'

import { releaseSteps } from './check-release.mjs'

test('release gate orders canonical state, phase closure, strict evidence, and final diff checks', () => {
  assert.deepEqual(
    releaseSteps.map(({ id }) => id),
    ['release-tree', 'planning-state', 'phase5-closure', 'phase1', 'phase2', 'phase3', 'phase4-release', 'phase5', 'release-tree-final', 'planning-state-final', 'diff-check'],
  )
  assert.deepEqual(releaseSteps[6]?.args, ['scripts/check-phase4.mjs', '--release-child', '--strict-external'])
  assert.deepEqual(releaseSteps.at(-1)?.args, ['diff', '--check'])
})
