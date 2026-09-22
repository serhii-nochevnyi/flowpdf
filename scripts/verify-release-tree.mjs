#!/usr/bin/env node

import { execFileSync } from 'node:child_process'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

export const projectRoot = resolve(import.meta.dirname, '..')

export function assertCleanTree(statusOutput) {
  if (String(statusOutput).trim() !== '') {
    throw new Error('release requires a clean tracked and untracked working tree')
  }
  return true
}

export function runReleaseTreeCheck({ output = process.stdout, root = projectRoot } = {}) {
  const status = execFileSync('git', ['status', '--porcelain', '--untracked-files=all'], {
    cwd: root,
    encoding: 'utf8',
  })
  try {
    assertCleanTree(status)
  } catch (error) {
    output.write(`${error instanceof Error ? error.message : String(error)}\n`)
    return 1
  }
  output.write('Release tree is clean\n')
  return 0
}

const isMain = process.argv[1] !== undefined && resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))

if (isMain) process.exitCode = runReleaseTreeCheck()
