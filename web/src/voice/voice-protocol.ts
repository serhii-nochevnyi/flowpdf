import type {
  DirectionalSelectionDto,
  InlineMarkDto,
} from '../editor/editor-store.js'

export const VOICE_PROTOCOL_VERSION = 1 as const

export type VoiceLocaleDto = 'uk-UA' | 'en-US'

export type VoiceMutationFamilyDto =
  | 'replaceSelection'
  | 'setInlineMark'
  | 'insertPageBreak'
  | 'removePageBreak'
  | 'setField'

export type VoiceRiskDto = 'text' | 'formatting' | 'structure' | 'compatibility'

export type VoiceConfirmationDto = 'none' | 'explicit' | 'conditional'

export type VoiceUndoDto = 'reversible'

export interface VoiceCapabilityDto {
  readonly family: VoiceMutationFamilyDto | null
  readonly commandType: string
  readonly intent: string
  readonly risk: VoiceRiskDto
  readonly confirmation: VoiceConfirmationDto
  readonly undo: VoiceUndoDto | null
}

export type VoiceActionDto =
  | { readonly type: 'undo' }
  | { readonly type: 'redo' }
  | { readonly type: 'deleteSelection' }
  | { readonly type: 'setInlineMark'; readonly mark: InlineMarkDto }
  | { readonly type: 'insertPageBreak' }
  | { readonly type: 'removePageBreak'; readonly pageBreakId: string }
  | {
      readonly type: 'navigateField'
      readonly direction: 'next' | 'previous'
    }
  | { readonly type: 'clearField'; readonly fieldId: string }

export interface VoiceIntentDto {
  readonly protocolVersion: typeof VOICE_PROTOCOL_VERSION
  readonly locale: VoiceLocaleDto
  readonly sourceRevision: number
  readonly selection: DirectionalSelectionDto | null
  readonly action: VoiceActionDto
  readonly capability: VoiceCapabilityDto
}

export interface AcceptedVoiceIntentDto extends VoiceIntentDto {
  /** The source hash is retained by the browser capture, never spoken back by Rust. */
  readonly sourceHash: string
}

export interface VoiceSourceCaptureDto {
  readonly sourceRevision: number
  readonly sourceHash: string
  readonly selection: DirectionalSelectionDto
}

export interface VoiceCommandCaptureDto extends VoiceSourceCaptureDto {
  readonly locale: VoiceLocaleDto
  readonly transcript: string
  readonly activeFieldId?: string
}

export interface VoiceDictationCaptureDto extends VoiceSourceCaptureDto {
  readonly text: string
}

export interface VoiceResolverResponseDto {
  readonly protocolVersion: number
  readonly ok: boolean
  readonly intent: VoiceIntentDto | null
  readonly error: { readonly code: string } | null
}

export type VoiceDispatchOutcome =
  | {
      readonly kind: 'committed'
      readonly revision: number
      readonly canonicalHash: string
    }
  | { readonly kind: 'rejected'; readonly code: string }
