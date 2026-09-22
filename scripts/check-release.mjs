#!/usr/bin/env node

import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

import { projectRoot as phaseFiveRoot } from './check-phase5.mjs'
import { runBoundedStep, writeFailureDetails } from './phase-gate-runner.mjs'

export const projectRoot = phaseFiveRoot

export const releaseSteps = Object.freeze([
  Object.freeze({ id: 'release-tree', command: process.execPath, args: ['scripts/verify-release-tree.mjs'] }),
  Object.freeze({ id: 'planning-state', command: process.execPath, args: ['scripts/verify-planning-state.mjs'] }),
  Object.freeze({ id: 'phase5-closure', command: process.execPath, args: ['scripts/check-phase5.mjs', '--preflight'] }),
  Object.freeze({ id: 'phase1', command: process.execPath, args: ['scripts/check-phase1.mjs'] }),
  Object.freeze({ id: 'phase2', command: process.execPath, args: ['scripts/check-phase2.mjs', '--release-child'] }),
  Object.freeze({ id: 'phase3', command: process.execPath, args: ['scripts/check-phase3.mjs', '--release-child'] }),
  Object.freeze({ id: 'phase4-release', command: process.execPath, args: ['scripts/check-phase4.mjs', '--release-child', '--strict-external'] }),
  Object.freeze({ id: 'phase5', command: process.execPath, args: ['scripts/check-phase5.mjs'] }),
  Object.freeze({ id: 'release-tree-final', command: process.execPath, args: ['scripts/verify-release-tree.mjs'] }),
  Object.freeze({ id: 'planning-state-final', command: process.execPath, args: ['scripts/verify-planning-state.mjs'] }),
  Object.freeze({ id: 'diff-check', command: 'git', args: ['diff', '--check'] }),
])

export function runReleaseGate({ output = process.stdout, steps = releaseSteps } = {}) {
  output.write('Release gate started\n')
  for (const step of steps) {
    output.write(`[${step.id}] start\n`)
    const { diagnostic, details } = runBoundedStep({
      id: step.id,
      command: step.command,
      args: step.args,
      cwd: projectRoot,
      env: step.env ?? process.env,
    })
    if (diagnostic.status === 'fail') {
      writeFailureDetails(output, step.id, details)
      output.write(`[${step.id}] fail exit=${diagnostic.exitCode}\n`)
      return diagnostic.exitCode
    }
    output.write(`[${step.id}] pass\n`)
  }
  output.write('Release gate passed\n')
  return 0
}

const isMain = process.argv[1] !== undefined && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))

if (isMain) process.exitCode = runReleaseGate()
