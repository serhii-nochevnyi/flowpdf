export const phaseTwoExternalTarget = 'Microsoft Edge on Windows with a Windows screen reader'

export function validateExternalAtEvidence(evidence) {
  if (evidence?.target !== phaseTwoExternalTarget) {
    throw new Error('external AT evidence target is not the required checkpoint')
  }

  if (evidence.status === 'unavailable') {
    if (evidence.substitutionForbidden?.length !== 2) {
      throw new Error('external evidence must prohibit the two local substitutions')
    }
    if (evidence.observed !== false || evidence.closure !== 'outstanding') {
      throw new Error('external Edge/Windows AT evidence must remain unavailable/outstanding')
    }
    if (/\b(pass|passed|complete|closed)\b/i.test(JSON.stringify(evidence))) {
      throw new Error('external AT evidence contains a fabricated pass/completion claim')
    }
    return evidence
  }

  if (
    evidence.status === 'observed' &&
    evidence.closure === 'closed' &&
    evidence.observed === true &&
    /Microsoft Edge on Windows/i.test(String(evidence.environment?.browser)) &&
    /Windows screen.?reader/i.test(String(evidence.environment?.screenReader))
  ) {
    return evidence
  }

  throw new Error('external Edge/Windows AT evidence must be unavailable/outstanding or observed/closed')
}
