import {
  MAX_COMPOSITION_UTF8_BYTES,
  MAX_COMPOSITION_UTF16_UNITS,
  validateInputPayload,
} from '../editor/input-adapter.js'
import type {
  DirectionalSelectionDto,
  EditorAcceptedSnapshot,
} from '../editor/editor-store.js'
import type {
  AcceptedVoiceIntentDto,
  VoiceActionDto,
  VoiceCapabilityDto,
  VoiceCommandCaptureDto,
  VoiceDictationCaptureDto,
  VoiceIntentDto,
  VoiceLocaleDto,
  VoiceMutationFamilyDto,
  VoiceResolverResponseDto,
} from './voice-protocol.js'
import { VOICE_PROTOCOL_VERSION } from './voice-protocol.js'

export interface VoiceCommandWasmBoundary {
  readonly resolve_voice_command: (requestJson: string) => string
}

export class VoiceBridgeError extends Error {
  constructor(readonly code: string) {
    super(code)
    this.name = 'VoiceBridgeError'
  }
}

export function voiceLocaleForEditorLocale(locale: 'uk' | 'en'): VoiceLocaleDto {
  return locale === 'uk' ? 'uk-UA' : 'en-US'
}

export function createVoiceCommandCapture(
  accepted: EditorAcceptedSnapshot,
  locale: VoiceLocaleDto,
  transcript: string,
  selection: DirectionalSelectionDto = accepted.editor.session.selection,
  activeFieldId?: string,
): VoiceCommandCaptureDto {
  return {
    locale,
    transcript,
    sourceRevision: accepted.session.revision,
    sourceHash: accepted.session.canonicalHash,
    selection,
    ...(activeFieldId === undefined ? {} : { activeFieldId }),
  }
}

export function createVoiceDictationCapture(
  accepted: EditorAcceptedSnapshot,
  text: string,
  selection: DirectionalSelectionDto = accepted.editor.session.selection,
): VoiceDictationCaptureDto {
  return {
    text,
    sourceRevision: accepted.session.revision,
    sourceHash: accepted.session.canonicalHash,
    selection,
  }
}

/** Returns a bounded input error without retaining the speech payload. */
export function validateVoiceDictationCapture(
  capture: VoiceDictationCaptureDto,
): string | null {
  if (capture.text.trim().length === 0) return 'FLOW_VOICE_DICTATION_EMPTY'
  if (
    capture.text.length > MAX_COMPOSITION_UTF16_UNITS ||
    capture.text.length > MAX_COMPOSITION_UTF8_BYTES
  ) {
    return 'FLOW_VOICE_DICTATION_LIMIT'
  }
  return validateInputPayload(capture.text, 'composition')
}

/**
 * Validates the Rust response and binds its intent to the captured browser
 * source identity. The raw transcript is used only to make the request and
 * is never included in a thrown error or returned DTO.
 */
export function resolveVoiceCommand(
  boundary: VoiceCommandWasmBoundary,
  capture: VoiceCommandCaptureDto,
): AcceptedVoiceIntentDto {
  validateCommandCapture(capture)
  let serialized: string
  try {
    serialized = boundary.resolve_voice_command(
      JSON.stringify({
        protocolVersion: VOICE_PROTOCOL_VERSION,
        locale: capture.locale,
        transcript: capture.transcript,
        sourceRevision: capture.sourceRevision,
        selection: capture.selection,
        ...(capture.activeFieldId === undefined
          ? {}
          : { activeFieldId: capture.activeFieldId }),
      }),
    )
  } catch {
    throw new VoiceBridgeError('FLOW_VOICE_BOUNDARY_FAILED')
  }

  const response = parseResponse(serialized)
  if (!response.ok) {
    throw new VoiceBridgeError(response.error?.code ?? 'FLOW_VOICE_RESPONSE_REJECTED')
  }
  if (response.intent === null || !isVoiceIntent(response.intent)) {
    throw new VoiceBridgeError('FLOW_VOICE_RESPONSE_DECODE')
  }
  const intent = response.intent
  if (
    intent.protocolVersion !== VOICE_PROTOCOL_VERSION ||
    intent.locale !== capture.locale ||
    intent.sourceRevision !== capture.sourceRevision ||
    !sameSelection(intent.selection, capture.selection)
  ) {
    throw new VoiceBridgeError('FLOW_VOICE_SOURCE_MISMATCH')
  }
  const intentError = validateIntentShape(intent)
  if (intentError !== null) throw new VoiceBridgeError(intentError)
  return { ...intent, sourceHash: capture.sourceHash }
}

/** Revalidates a typed intent before a controller dispatch from browser code. */
export function validateAcceptedVoiceIntent(
  intent: AcceptedVoiceIntentDto,
): string | null {
  if (intent.sourceHash.trim().length === 0) return 'FLOW_VOICE_SOURCE_INVALID'
  return validateIntentShape(intent)
}

function validateIntentShape(intent: VoiceIntentDto): string | null {
  if (intent.selection === null) return 'FLOW_VOICE_SELECTION_REQUIRED'
  if (!hasExpectedCapability(intent.action, intent.capability)) {
    return 'FLOW_VOICE_CAPABILITY_MISMATCH'
  }
  if (
    intent.action.type === 'removePageBreak' &&
    intent.action.pageBreakId !== intent.selection.anchor.nodeId
  ) {
    return 'FLOW_VOICE_SELECTION_MISMATCH'
  }
  return null
}

function validateCommandCapture(capture: VoiceCommandCaptureDto): void {
  if (!isSafeRevision(capture.sourceRevision) || capture.sourceHash.trim().length === 0) {
    throw new VoiceBridgeError('FLOW_VOICE_SOURCE_INVALID')
  }
  if (!isSelection(capture.selection)) {
    throw new VoiceBridgeError('FLOW_VOICE_SELECTION_INVALID')
  }
  if (capture.transcript.trim().length === 0) {
    throw new VoiceBridgeError('FLOW_VOICE_TRANSCRIPT_EMPTY')
  }
}

function parseResponse(serialized: string): VoiceResolverResponseDto {
  let value: unknown
  try {
    value = JSON.parse(serialized)
  } catch {
    throw new VoiceBridgeError('FLOW_VOICE_RESPONSE_DECODE')
  }
  if (!isRecord(value)) throw new VoiceBridgeError('FLOW_VOICE_RESPONSE_DECODE')
  if (
    !Number.isSafeInteger(value.protocolVersion) ||
    value.protocolVersion !== VOICE_PROTOCOL_VERSION ||
    typeof value.ok !== 'boolean'
  ) {
    throw new VoiceBridgeError('FLOW_VOICE_PROTOCOL_UNSUPPORTED')
  }
  if (value.ok) {
    if (value.error !== null || !isRecord(value.intent)) {
      throw new VoiceBridgeError('FLOW_VOICE_RESPONSE_DECODE')
    }
    return {
      protocolVersion: value.protocolVersion,
      ok: true,
      intent: value.intent as unknown as VoiceIntentDto,
      error: null,
    }
  }
  if (value.intent !== null || !isRecord(value.error) || typeof value.error.code !== 'string') {
    throw new VoiceBridgeError('FLOW_VOICE_RESPONSE_DECODE')
  }
  return {
    protocolVersion: value.protocolVersion,
    ok: false,
    intent: null,
    error: { code: value.error.code },
  }
}

function isVoiceIntent(value: unknown): value is VoiceIntentDto {
  if (!isRecord(value)) return false
  return (
    value.protocolVersion === VOICE_PROTOCOL_VERSION &&
    isVoiceLocale(value.locale) &&
    isSafeRevision(value.sourceRevision) &&
    (value.selection === null || isSelection(value.selection)) &&
    isVoiceAction(value.action) &&
    isVoiceCapability(value.capability)
  )
}

function isVoiceAction(value: unknown): value is VoiceActionDto {
  if (!isRecord(value) || typeof value.type !== 'string') return false
  switch (value.type) {
    case 'undo':
    case 'redo':
    case 'deleteSelection':
    case 'insertPageBreak':
      return Object.keys(value).length === 1
    case 'setInlineMark':
      return isRecord(value.mark) && isInlineMark(value.mark)
    case 'removePageBreak':
      return typeof value.pageBreakId === 'string' && value.pageBreakId.length > 0
    case 'navigateField':
      return value.direction === 'next' || value.direction === 'previous'
    case 'clearField':
      return typeof value.fieldId === 'string' && value.fieldId.length > 0
    default:
      return false
  }
}

function isVoiceCapability(value: unknown): value is VoiceCapabilityDto {
  if (!isRecord(value)) return false
  return (
    (value.family === null || isVoiceFamily(value.family)) &&
    typeof value.commandType === 'string' &&
    value.commandType.length > 0 &&
    typeof value.intent === 'string' &&
    value.intent.length > 0 &&
    isVoiceRisk(value.risk) &&
    isVoiceConfirmation(value.confirmation) &&
    (value.undo === null || value.undo === 'reversible')
  )
}

function hasExpectedCapability(
  action: VoiceActionDto,
  capability: VoiceCapabilityDto,
): boolean {
  const expected = expectedCapability(action)
  return (
    capability.family === expected.family &&
    capability.commandType === expected.commandType &&
    capability.intent === expected.intent &&
    capability.risk === expected.risk &&
    capability.confirmation === expected.confirmation &&
    capability.undo === expected.undo
  )
}

function expectedCapability(action: VoiceActionDto): VoiceCapabilityDto {
  switch (action.type) {
    case 'undo':
      return typedCapability('undo', 'editor.intent.undo', 'text')
    case 'redo':
      return typedCapability('redo', 'editor.intent.redo', 'text')
    case 'deleteSelection':
      return catalogCapability('replaceSelection', 'replaceSelection', 'editor.intent.replaceSelection', 'text', 'explicit')
    case 'setInlineMark':
      return catalogCapability('setInlineMark', 'setInlineMark', 'editor.intent.setInlineMark', 'formatting', 'none')
    case 'insertPageBreak':
      return catalogCapability('insertPageBreak', 'insertPageBreak', 'editor.intent.insertPageBreak', 'structure', 'none')
    case 'removePageBreak':
      return catalogCapability('removePageBreak', 'removePageBreak', 'editor.intent.removePageBreak', 'structure', 'explicit')
    case 'navigateField':
      return typedCapability('navigateField', 'editor.intent.navigateField', 'text')
    case 'clearField':
      return catalogCapability('setField', 'setField', 'editor.intent.setField', 'compatibility', 'explicit')
  }
}

function catalogCapability(
  family: Exclude<VoiceMutationFamilyDto, null>,
  commandType: string,
  intent: string,
  risk: VoiceCapabilityDto['risk'],
  confirmation: VoiceCapabilityDto['confirmation'],
): VoiceCapabilityDto {
  return { family, commandType, intent, risk, confirmation, undo: 'reversible' }
}

function typedCapability(
  commandType: string,
  intent: string,
  risk: VoiceCapabilityDto['risk'],
): VoiceCapabilityDto {
  return {
    family: null,
    commandType,
    intent,
    risk,
    confirmation: 'none',
    undo: null,
  }
}

function isInlineMark(value: Record<string, unknown>): boolean {
  if (typeof value.kind !== 'string') return false
  switch (value.kind) {
    case 'bold':
    case 'italic':
    case 'underline':
      return typeof value.value === 'boolean'
    case 'fontFamily':
      return value.value === null || isRecord(value.value)
    case 'fontSize':
      return value.value === null || isSafeRevision(value.value)
    case 'color':
      return value.value === null || typeof value.value === 'string'
    case 'language':
      return value.value === null || value.value === 'uk-UA' || value.value === 'en-US'
    default:
      return false
  }
}

function isSelection(value: unknown): value is DirectionalSelectionDto {
  if (!isRecord(value)) return false
  return isPosition(value.anchor) && isPosition(value.focus)
}

function isPosition(value: unknown): boolean {
  if (!isRecord(value)) return false
  return (
    typeof value.nodeId === 'string' &&
    value.nodeId.length > 0 &&
    isSafeRevision(value.utf16Offset) &&
    (value.affinity === 'forward' || value.affinity === 'backward')
  )
}

function sameSelection(
  left: DirectionalSelectionDto | null,
  right: DirectionalSelectionDto,
): boolean {
  return (
    left !== null &&
    left.anchor.nodeId === right.anchor.nodeId &&
    left.anchor.utf16Offset === right.anchor.utf16Offset &&
    left.anchor.affinity === right.anchor.affinity &&
    left.focus.nodeId === right.focus.nodeId &&
    left.focus.utf16Offset === right.focus.utf16Offset &&
    left.focus.affinity === right.focus.affinity
  )
}

function isVoiceLocale(value: unknown): value is VoiceLocaleDto {
  return value === 'uk-UA' || value === 'en-US'
}

function isVoiceFamily(value: unknown): value is VoiceMutationFamilyDto {
  return (
    value === 'replaceSelection' ||
    value === 'setInlineMark' ||
    value === 'insertPageBreak' ||
    value === 'removePageBreak' ||
    value === 'setField'
  )
}

function isVoiceRisk(value: unknown): value is VoiceCapabilityDto['risk'] {
  return value === 'text' || value === 'formatting' || value === 'structure' || value === 'compatibility'
}

function isVoiceConfirmation(value: unknown): value is VoiceCapabilityDto['confirmation'] {
  return value === 'none' || value === 'explicit' || value === 'conditional'
}

function isSafeRevision(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
}
