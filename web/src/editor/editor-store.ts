import type {
  AuditRecordDto,
  HistoryStateDto,
  LogicalPositionDto,
} from '../../persistence/indexeddb-store.js'

export type EditorLocale = 'uk' | 'en'

export type FormattingStateDto = 'on' | 'off' | 'mixed'

export type RunLanguageDto = 'uk-UA' | 'en-US'

export type AlignmentDto = 'start' | 'center' | 'end' | 'justify'

export type ListKindDto = 'none' | 'ordered' | 'unordered'

export type FontFamilyIdDto = 'notoSans' | 'notoSerif' | 'notoSansMono'

export type FontFamilyDto =
  | { readonly kind: 'known'; readonly id: FontFamilyIdDto }
  | { readonly kind: 'legacyUnknown'; readonly original: string }

export interface MarkSetDto {
  readonly bold: boolean
  readonly italic: boolean
  readonly underline: boolean
  readonly fontFamily: FontFamilyDto | null
  readonly fontSizeMillipoints: number | null
  readonly color: readonly [number, number, number] | null
  readonly language: RunLanguageDto | null
}

export interface FormattingProjectionDto {
  readonly bold: FormattingStateDto
  readonly italic: FormattingStateDto
  readonly underline: FormattingStateDto
  readonly fontFamily: FontFamilyDto | null
  readonly fontSizeMillipoints: number | null
  readonly color: readonly [number, number, number] | null
  readonly language: RunLanguageDto | null
  readonly blockStyle: BlockStyleDto | null
  readonly alignment: AlignmentDto | null
  readonly spacingBeforeMillipoints: number | null
  readonly spacingAfterMillipoints: number | null
  readonly listKind: ListKindDto | null
  readonly listItemId: string | null
}

export type BlockStyleDto =
  | { readonly kind: 'paragraph' }
  | { readonly kind: 'heading'; readonly level: number }

export interface BlockAttributesDto {
  readonly alignment?: AlignmentDto | null
  readonly spacingBeforeMillipoints?: number | null
  readonly spacingAfterMillipoints?: number | null
}

export type InlineMarkDto =
  | { readonly kind: 'bold'; readonly value: boolean }
  | { readonly kind: 'italic'; readonly value: boolean }
  | { readonly kind: 'underline'; readonly value: boolean }
  | { readonly kind: 'fontFamily'; readonly value: FontFamilyDto | null }
  | { readonly kind: 'fontSize'; readonly value: number | null }
  | { readonly kind: 'color'; readonly value: string | null }
  | { readonly kind: 'language'; readonly value: RunLanguageDto | null }

export interface DirectionalSelectionDto {
  readonly anchor: LogicalPositionDto
  readonly focus: LogicalPositionDto
}

export interface EditorSessionStateDto {
  readonly documentId: string
  readonly revision: number
  readonly sessionGeneration: number
  readonly selection: DirectionalSelectionDto
  readonly pendingMarks: MarkSetDto
  readonly formatting: FormattingProjectionDto
  readonly capabilities: readonly EditorCapabilityDto[]
}

export interface EditorCapabilityDto {
  readonly name: string
  readonly enabled: boolean
  readonly reasonKey?: string | null
}

export interface EditorTextSpanDto {
  readonly startUtf16: number
  readonly endUtf16: number
}

export interface EditorBlockViewDto {
  readonly kind: 'paragraph' | 'heading' | 'atomic'
  readonly nodeId: string
  readonly text?: string
  readonly level?: number
  readonly nodeKind?: string
  readonly spans?: readonly EditorTextSpanDto[]
  readonly children?: readonly EditorBlockViewDto[]
}

export interface EditorDocumentViewDto {
  readonly documentId: string
  readonly revision: number
  readonly blocks: readonly EditorBlockViewDto[]
}

export interface EditorViewDto {
  readonly documentId: string
  readonly revision: number
  readonly sessionGeneration: number
  readonly selection: DirectionalSelectionDto
  readonly pendingMarks: MarkSetDto
  readonly formatting: FormattingProjectionDto
  readonly capabilities: readonly EditorCapabilityDto[]
  readonly document: EditorDocumentViewDto
}

export interface EditorSessionResponseDto {
  readonly session: EditorSessionStateDto
  readonly view: EditorViewDto
}

export interface SessionDto {
  readonly canonicalJson: string
  readonly canonicalHash: string
  readonly documentId: string
  readonly revision: number
  readonly nextCommandTarget: LogicalPositionDto | null
  readonly history: HistoryStateDto
}

export interface RevisionProvenanceDto {
  readonly documentId: string
  readonly revision: number
  readonly schemaVersion: number
  readonly canonicalHash: string
  readonly lineage: unknown
  readonly engine: unknown
  readonly sourceHashes: readonly string[]
  readonly previewExportProvenance: string
}

export interface InspectorViewDto {
  readonly documentId: string
  readonly schemaVersion: number
  readonly revision: number
  readonly canonicalHash: string
  readonly locale: string
  readonly contentNodeCount: number
  readonly fieldCount: number
  readonly assetCount: number
  readonly revisionProvenance: RevisionProvenanceDto
  readonly audit: readonly AuditRecordDto[]
}

export interface EditorAcceptedSnapshot {
  readonly session: SessionDto
  readonly editor: EditorSessionResponseDto
  readonly view: InspectorViewDto
}

export type EditorAppPhase = 'loading' | 'empty' | 'pending' | 'ready' | 'error'

export interface EditorAppSnapshot {
  readonly phase: EditorAppPhase
  readonly accepted: EditorAcceptedSnapshot | null
  readonly status: string
  readonly errorCode: string | null
}

export class EditorStore {
  private readonly listeners = new Set<() => void>()
  private acceptedValue: EditorAcceptedSnapshot | null = null
  private snapshotValue: EditorAppSnapshot

  constructor(initialStatus: string) {
    this.snapshotValue = Object.freeze({
      phase: 'loading',
      accepted: null,
      status: initialStatus,
      errorCode: null,
    })
  }

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    return () => {
      this.listeners.delete(listener)
    }
  }

  readonly getSnapshot = (): EditorAppSnapshot => this.snapshotValue

  accepted(): EditorAcceptedSnapshot | null {
    return this.acceptedValue
  }

  publishPending(status: string): void {
    this.publish({
      phase: 'pending',
      accepted: this.acceptedValue,
      status,
      errorCode: null,
    })
  }

  publishEmpty(): void {
    this.acceptedValue = null
    this.publish({
      phase: 'empty',
      accepted: null,
      status: '',
      errorCode: null,
    })
  }

  publishAccepted(accepted: EditorAcceptedSnapshot, status: string): void {
    this.acceptedValue = Object.freeze(accepted)
    this.publish({
      phase: 'ready',
      accepted: this.acceptedValue,
      status,
      errorCode: null,
    })
  }

  publishError(errorCode: string): void {
    this.publish({
      phase: 'error',
      accepted: this.acceptedValue,
      status: '',
      errorCode,
    })
  }

  dispose(): void {
    this.listeners.clear()
  }

  private publish(snapshot: EditorAppSnapshot): void {
    this.snapshotValue = Object.freeze(snapshot)
    for (const listener of this.listeners) listener()
  }
}
