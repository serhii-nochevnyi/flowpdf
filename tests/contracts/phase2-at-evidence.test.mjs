import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import test from 'node:test'

import { validateExternalAtEvidence } from '../../scripts/phase2-at-evidence.mjs'

const projectRoot = resolve(import.meta.dirname, '../..')
function jsonBlock(path) {
  const markdown = readFileSync(path, 'utf8')
  const match = markdown.match(/```json\n([\s\S]*?)\n```/)
  if (match === null) throw new Error(`missing JSON evidence block: ${path}`)
  return JSON.parse(match[1])
}

export function loadPhaseTwoAtEvidence(root = projectRoot) {
  return {
    local: jsonBlock(resolve(root, 'tests/manual/phase2-local-voiceover.md')),
    external: jsonBlock(resolve(root, 'tests/manual/phase2-edge-windows-screen-reader.md')),
  }
}

export function validatePhaseTwoAtEvidence(evidence) {
  if (evidence?.local?.phase !== 'FLOWPDF-02-accessible-rich-text-editing') {
    throw new Error('local AT evidence has the wrong phase')
  }
  if (!['observed', 'not-run', 'unavailable'].includes(evidence.local.status)) {
    throw new Error('local AT evidence has an invalid status')
  }
  if (evidence.local.status === 'not-run' && evidence.local.observed !== false) {
    throw new Error('not-run local AT evidence cannot claim observed=true')
  }
  if (evidence.local.status === 'unavailable' && evidence.local.observed !== false) {
    throw new Error('unavailable local AT evidence cannot claim observed=true')
  }
  if (
    evidence.local.status === 'observed' &&
    (
      evidence.local.observed !== true ||
      !/VoiceOver/i.test(String(evidence.local.target)) ||
      !/VoiceOver/i.test(String(evidence.local.environment?.browser))
    )
  ) {
    throw new Error('observed local AT evidence must name an actually exercised VoiceOver target')
  }
  if (/\b(pass|passed|complete|closed)\b/i.test(JSON.stringify(evidence.local))) {
    throw new Error('local AT evidence contains an unqualified pass/completion claim')
  }
  validateExternalAtEvidence(evidence.external)
  return true
}

test('local VoiceOver evidence is observed or explicitly not-run, never inferred', () => {
  const evidence = loadPhaseTwoAtEvidence()
  validatePhaseTwoAtEvidence(evidence)
  assert.ok(['observed', 'not-run', 'unavailable'].includes(evidence.local.status))
  assert.equal(evidence.local.environment.host, 'macOS arm64 workspace')
})

test('Edge/Windows plus Windows screen-reader evidence remains unavailable/outstanding', () => {
  const evidence = loadPhaseTwoAtEvidence()
  validatePhaseTwoAtEvidence(evidence)
  assert.equal(evidence.external.status, 'unavailable')
  assert.equal(evidence.external.closure, 'outstanding')
})

test('AT evidence rejects a fabricated local pass claim', () => {
  const evidence = loadPhaseTwoAtEvidence()
  evidence.local.status = 'passed'
  assert.throws(() => validatePhaseTwoAtEvidence(evidence), /invalid status|pass\/completion/)
})

test('AT evidence rejects substitution of Chromium or macOS VoiceOver for Windows UAT', () => {
  const evidence = loadPhaseTwoAtEvidence()
  evidence.external.status = 'passed'
  evidence.external.closure = 'complete'
  evidence.external.observed = true
  assert.throws(() => validatePhaseTwoAtEvidence(evidence), /unavailable\/outstanding/)

  const substituted = loadPhaseTwoAtEvidence()
  substituted.external.substitutionForbidden = ['Chromium on macOS passed the external checkpoint.']
  assert.throws(() => validatePhaseTwoAtEvidence(substituted), /two local substitutions|substitute/)
})

test('AT evidence requires a real platform record before allowing observed local status', () => {
  const evidence = loadPhaseTwoAtEvidence()
  evidence.local.status = 'observed'
  evidence.local.observed = true
  evidence.local.environment.browser = 'Chromium on macOS'
  assert.throws(() => validatePhaseTwoAtEvidence(evidence), /pass\/completion|VoiceOver|inferred/)
})

test('AT evidence accepts a recorded Edge/Windows observation only after a closed target run', () => {
  const evidence = loadPhaseTwoAtEvidence()
  evidence.external.status = 'observed'
  evidence.external.observed = true
  evidence.external.closure = 'closed'
  evidence.external.environment.browser = 'Microsoft Edge on Windows'
  evidence.external.environment.screenReader = 'Windows screen reader'
  assert.doesNotThrow(() => validatePhaseTwoAtEvidence(evidence))
})
