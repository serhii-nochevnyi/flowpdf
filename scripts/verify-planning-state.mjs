#!/usr/bin/env node

import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

export const projectRoot = resolve(import.meta.dirname, '..')

export function inspectPlanningState({
  stateMarkdown,
  actualHead,
  stateJsonExists = false,
  stateJsonTracked = true,
  latestCompletedPlan = null,
  currentPlan = null,
}) {
  const issues = []
  const stateHead = /^state_head:\s*"?([0-9a-f]+)"?/im.exec(stateMarkdown)?.[1]
  if (stateHead === undefined) {
    issues.push('STATE.md has no state_head')
  } else if (!(actualHead.startsWith(stateHead) || stateHead.startsWith(actualHead))) {
    issues.push(`STATE.md state_head ${stateHead} does not match HEAD ${actualHead}`)
  }

  if (stateJsonExists && !stateJsonTracked) {
    issues.push('.planning/state.json exists but is untracked; remove it or make it canonical before release')
  }

  if (latestCompletedPlan !== null && currentPlan !== null && latestCompletedPlan !== currentPlan) {
    issues.push(`STATE.md current plan ${currentPlan} does not match latest completed plan ${latestCompletedPlan}`)
  }

  return issues
}

function gitOutput(root, args) {
  return execFileSync('git', args, {
    cwd: root,
    encoding: 'utf8',
    stdio: ['ignore', 'pipe', 'ignore'],
  }).trim()
}

function isTracked(root, relativePath) {
  try {
    gitOutput(root, ['ls-files', '--error-unmatch', '--', relativePath])
    return true
  } catch {
    return false
  }
}

function latestCompletedPlan(root) {
  const phaseRoot = resolve(root, '.planning/phases')
  const plans = []
  for (const phaseName of readdirSync(phaseRoot)) {
    const phasePath = resolve(phaseRoot, phaseName)
    let names
    try {
      names = readdirSync(phasePath)
    } catch {
      continue
    }
    for (const name of names) {
      const match = /^(\d{2})-(\d{2})-SUMMARY\.md$/.exec(name)
      if (match === null) continue
      const summary = readFileSync(resolve(phasePath, name), 'utf8')
      if (/^(?:\*\*Status:\*\*|status:)\s+complete\b/im.test(summary)) {
        plans.push(`${match[1]}-${match[2]}`)
      }
    }
  }
  return plans.sort().at(-1) ?? null
}

function currentPlan(stateMarkdown) {
  const phase = /^current_phase:\s*"?(\d+)"?/im.exec(stateMarkdown)?.[1]
  const plan = /^Plan:\s*(\d{1,2})\b/im.exec(stateMarkdown)?.[1]
  if (phase === undefined || plan === undefined) return null
  return `${String(Number(phase)).padStart(2, '0')}-${String(Number(plan)).padStart(2, '0')}`
}

export function loadPlanningState(root = projectRoot) {
  const stateMarkdown = readFileSync(resolve(root, '.planning/STATE.md'), 'utf8')
  const stateJsonPath = resolve(root, '.planning/state.json')
  return {
    issues: inspectPlanningState({
      stateMarkdown,
      actualHead: gitOutput(root, ['rev-parse', 'HEAD']),
      stateJsonExists: existsSync(stateJsonPath),
      stateJsonTracked: isTracked(root, '.planning/state.json'),
      latestCompletedPlan: latestCompletedPlan(root),
      currentPlan: currentPlan(stateMarkdown),
    }),
  }
}

export function runPlanningStateCheck({ output = process.stdout, root = projectRoot } = {}) {
  const { issues } = loadPlanningState(root)
  if (issues.length === 0) {
    output.write('Planning state is synchronized\n')
    return 0
  }
  output.write(`Planning state check failed (${issues.length})\n`)
  for (const issue of issues) output.write(`- ${issue}\n`)
  return 1
}

const isMain = process.argv[1] !== undefined && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))

if (isMain) process.exitCode = runPlanningStateCheck()
