import assert from 'node:assert/strict'
import test from 'node:test'

import { inspectPlanningState } from './verify-planning-state.mjs'

const synchronizedState = `---
current_phase: 5
state_head: 8af808c
---
Plan: 25 executed
`

test('planning state accepts a matching head and completed plan', () => {
  assert.deepEqual(
    inspectPlanningState({
      stateMarkdown: synchronizedState,
      actualHead: '8af808c1234567890',
      latestCompletedPlan: '05-25',
      currentPlan: '05-25',
    }),
    [],
  )
})

test('planning state reports stale head, untracked projection, and plan drift', () => {
  const issues = inspectPlanningState({
    stateMarkdown: synchronizedState,
    actualHead: 'deadbeef1234567890',
    stateJsonExists: true,
    stateJsonTracked: false,
    latestCompletedPlan: '05-26',
    currentPlan: '05-25',
  })
  assert.equal(issues.length, 3)
  assert.match(issues[0], /does not match HEAD/)
  assert.match(issues[1], /state\.json exists but is untracked/)
  assert.match(issues[2], /latest completed plan 05-26/)
})
