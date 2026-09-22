import {
  IndexedDbDocumentStore,
  StorageError,
  type AuditRecordDto,
  type AssetRecordDto,
  type DocumentHeadDto,
  type FormSessionIdentityDto,
  type HistoryStateDto,
  type LogicalPositionDto,
  type MigrationPersistenceCommitDto,
  type PersistenceCommitDto,
  type PlannedPersistenceCommitDto,
  type RecoveryImageDto,
  type RecoveryRecordsDto,
} from '../../persistence/indexeddb-store.js'
import {
  EditorStore,
  type DirectionalSelectionDto,
  type EditorAcceptedSnapshot,
  type EditorBlockViewDto,
  type EditorLocale,
  type FieldDescriptorDto,
  type EditorSessionResponseDto,
  type EditorSessionStateDto,
  type EditorViewDto,
  type InspectorViewDto,
  type SessionDto,
  type BlockAttributesDto,
  type BlockStyleDto,
  type InlineMarkDto,
  type ListKindDto,
  type MarkSetDto,
  type StructuralPlacementDto,
  type ImageAccessibilityDto,
  type ImageBlockViewDto,
} from './editor-store.js'
import type {
  AcceptedLayoutDto,
  LayoutRequestDto,
  LayoutSchedulerSnapshotDto,
} from '../layout/layout-protocol.js'
import type {
  LayoutScheduleOutcome,
} from '../layout/layout-worker.js'
import type {
  AcceptedPdfExportDto,
  PdfExportRequestDto,
  PdfExportSchedulerSnapshotDto,
} from '../pdf/pdf-protocol.js'
import type { PdfExportScheduleOutcome } from '../pdf/pdf-worker.js'
import type {
  AcceptedPdfReaderDto,
  PdfReaderRequestDto,
  PdfReaderSchedulerSnapshotDto,
} from '../pdf/pdf-protocol.js'
import type { PdfReaderScheduleOutcome } from '../pdf/pdf-reader-worker.js'
import type {
  AcceptedPdfReconstructionDto,
  PdfReconstructionRequestDto,
  PdfReconstructionAcceptedCandidateDto,
  PdfReconstructionSchedulerSnapshotDto,
  PdfReviewDecisionDto,
} from '../pdf/pdf-reconstruction-protocol.js'
import type {
  PdfReconstructionAcceptOutcome,
  PdfReconstructionScheduleOutcome,
} from '../pdf/pdf-reconstruction-worker.js'
import { createPdfExportRequest } from '../pdf/pdf-request.js'
import {
  resolveVoiceFieldTarget,
  type FormSessionWasmBoundary,
  type VoiceFormSessionTarget,
} from '../forms/form-session.js'
import {
  type FormProjectionSelectionDto,
  type FormProjectionWasmBoundary,
  validateFormProjectionSelection,
} from '../forms/form-projection.js'
import {
  createVoiceCommandCapture,
  createVoiceDictationCapture,
  resolveVoiceCommand as resolveVoiceIntent,
  validateAcceptedVoiceIntent,
  validateVoiceDictationCapture,
} from '../voice/voice-command.js'
import type {
  AcceptedVoiceIntentDto,
  VoiceDictationCaptureDto,
  VoiceDispatchOutcome,
} from '../voice/voice-protocol.js'

export type { EditorLocale }

export type SourceModality = 'ui' | 'keyboard' | 'voice' | 'api' | 'system'

export type StructuralCommandDto =
  | {
      readonly type: 'splitTextBlock'
      readonly nodeId: string
      readonly utf16Offset: number
      readonly newNodeId: string
    }
  | {
      readonly type: 'mergeTextBlocks'
      readonly firstNodeId: string
      readonly secondNodeId: string
    }
  | {
      readonly type: 'deleteSubtree'
      readonly nodeId: string
    }
  | {
      readonly type: 'insertPageBreak'
      readonly placement: StructuralPlacementDto
    }
  | {
      readonly type: 'removePageBreak'
      readonly pageBreakId: string
    }
  | {
      readonly type: 'insertTable'
      readonly placement: StructuralPlacementDto
      readonly rows: number
      readonly columns: number
      readonly headerRow: boolean
    }
  | {
      readonly type: 'addTableRow'
      readonly selection: DirectionalSelectionDto
    }
  | {
      readonly type: 'removeTableRow'
      readonly selection: DirectionalSelectionDto
    }
  | {
      readonly type: 'addTableColumn'
      readonly selection: DirectionalSelectionDto
    }
  | {
      readonly type: 'removeTableColumn'
      readonly selection: DirectionalSelectionDto
    }
  | {
      readonly type: 'setTableHeaderRow'
      readonly tableId: string
      readonly enabled: boolean
    }
  | {
      readonly type: 'removeTable'
      readonly tableId: string
      readonly confirmed: boolean
    }
  | {
      readonly type: 'insertField'
      readonly field: FieldDescriptorDto
    }
  | {
      readonly type: 'setField'
      readonly fieldId: string
      readonly field: FieldDescriptorDto
    }
  | {
      readonly type: 'removeField'
      readonly fieldId: string
      readonly confirmed: boolean
    }
  | {
      readonly type: 'moveField'
      readonly fieldId: string
      readonly targetIndex: number
    }
  | {
      readonly type: 'insertImage'
      readonly placement: StructuralPlacementDto
      readonly sessionId: string
      readonly receipt: string
      readonly accessibility: ImageAccessibilityDto
    }
  | {
      readonly type: 'replaceImage'
      readonly imageNodeId: string
      readonly sessionId: string
      readonly receipt: string
      readonly accessibility: ImageAccessibilityDto
    }
  | {
      readonly type: 'setImageAccessibility'
      readonly imageNodeId: string
      readonly accessibility: Exclude<ImageAccessibilityDto, { readonly kind: 'missingLegacy' }>
    }
  | {
      readonly type: 'removeImage'
      readonly imageNodeId: string
      readonly confirmed: boolean
    }

export type FormattingCommandDto =
  | {
      readonly type: 'setInlineMarks'
      readonly selection: DirectionalSelectionDto
      readonly marks: MarkSetDto
    }
  | {
      readonly type: 'setInlineMark'
      readonly selection: DirectionalSelectionDto
      readonly mark: InlineMarkDto
    }
  | {
      readonly type: 'setBlockAttributes'
      readonly selection: DirectionalSelectionDto
      readonly attributes: BlockAttributesDto
    }
  | {
      readonly type: 'setBlockStyle'
      readonly selection: DirectionalSelectionDto
      readonly style: BlockStyleDto
    }
  | {
      readonly type: 'setListKind'
      readonly selection: DirectionalSelectionDto
      readonly kind: ListKindDto
    }
  | {
      readonly type: 'continueListItem'
      readonly selection: DirectionalSelectionDto
    }
  | {
      readonly type: 'exitListItem'
      readonly selection: DirectionalSelectionDto
    }
  | {
      readonly type: 'indentListItem'
      readonly itemId: string
    }
  | {
      readonly type: 'outdentListItem'
      readonly itemId: string
    }

export type EditorCommandDto = StructuralCommandDto | FormattingCommandDto

export interface FormattingCommandTarget {
  setInlineMark(
    mark: InlineMarkDto,
    selection?: DirectionalSelectionDto,
    modality?: SourceModality,
  ): Promise<void>
  setBlockStyle(
    style: BlockStyleDto,
    selection?: DirectionalSelectionDto,
    modality?: SourceModality,
  ): Promise<void>
  setBlockAttributes(
    attributes: BlockAttributesDto,
    selection?: DirectionalSelectionDto,
    modality?: SourceModality,
  ): Promise<void>
  setListKind(
    kind: ListKindDto,
    selection?: DirectionalSelectionDto,
    modality?: SourceModality,
  ): Promise<void>
  indentListItem(itemId: string, modality?: SourceModality): Promise<void>
  outdentListItem(itemId: string, modality?: SourceModality): Promise<void>
}

export interface StructuralCommandTarget {
  snapshot(): { readonly phase: string }
  structuralCommand(command: StructuralCommandDto, modality?: SourceModality): Promise<void>
  insertImage?(
    bytes: Uint8Array,
    placement: StructuralPlacementDto,
    accessibility: Exclude<ImageAccessibilityDto, { readonly kind: 'missingLegacy' }>,
    modality?: SourceModality,
  ): Promise<void>
  replaceImage?(
    imageNodeId: string,
    bytes: Uint8Array,
    accessibility: Exclude<ImageAccessibilityDto, { readonly kind: 'missingLegacy' }>,
    modality?: SourceModality,
  ): Promise<void>
  setImageAccessibility?(
    imageNodeId: string,
    accessibility: Exclude<ImageAccessibilityDto, { readonly kind: 'missingLegacy' }>,
    modality?: SourceModality,
  ): Promise<void>
  imageSource?(contentHash: string): string | null
}

interface ErrorDto {
  readonly code: string
  readonly message: string
  readonly audit: AuditRecordDto | null
}

export interface ApiResponse<T> {
  readonly ok: boolean
  readonly value: T | null
  readonly error: ErrorDto | null
}

interface OperationResultDto {
  readonly session: SessionDto
  readonly view: InspectorViewDto
  readonly editor: EditorSessionResponseDto
  readonly commit: PersistenceCommitDto
}

interface RecoverResultDto {
  readonly session: SessionDto
  readonly view: InspectorViewDto
  readonly editor: EditorSessionResponseDto
}

interface AuditedRecoverResultDto {
  readonly recovered: RecoverResultDto
  readonly audit: AuditRecordDto
}

export interface AssetStageResponseDto {
  readonly receipt: string
  readonly contentHash: string
  readonly mediaType: string
  readonly byteLength: number
  readonly width: number
  readonly height: number
  readonly expiresAtUnixSeconds: number
}

interface MigrateDocumentResultDto {
  readonly canonicalJson: string
  readonly canonicalHash: string
  readonly revisionProvenance: {
    readonly documentId: string
    readonly revision: number
    readonly schemaVersion: number
    readonly canonicalHash: string
    readonly lineage: unknown
    readonly engine: unknown
    readonly sourceHashes: readonly string[]
    readonly previewExportProvenance: string
  }
  readonly report: {
    readonly sourceSchemaVersion: number
    readonly currentSchemaVersion: number
  }
  readonly editor: EditorSessionResponseDto
  readonly commit: MigrationPersistenceCommitDto | null
}

interface ApplyCommandRequestDto {
  readonly canonicalJson: string
  readonly history: HistoryStateDto
  readonly command: {
    readonly commandId: string
    readonly baseRevision: number
    readonly modality: SourceModality
    readonly issuedAt: string
    readonly kind:
      | {
          readonly type: 'replaceSelection'
          readonly selection: DirectionalSelectionDto
          readonly text: string
        }
      | StructuralCommandDto
      | FormattingCommandDto
      | { readonly type: 'undo' }
      | { readonly type: 'redo' }
  }
}

interface RecoveryAuditContextDto {
  readonly attemptId: string
  readonly documentId: string
  readonly expectedRevision: number
  readonly issuedAt: string
}

type StandaloneAuditDerivationDto = {
  readonly type: 'recovery'
  readonly auditContext: RecoveryAuditContextDto
}

type EditorSessionActionDto =
  | {
      readonly type: 'setSelection'
      readonly selection: DirectionalSelectionDto
    }
  | {
      readonly type: 'setPendingMarks'
      readonly marks: MarkSetDto
    }
  | {
      readonly type: 'setPendingMark'
      readonly mark: InlineMarkDto
    }

export interface WasmBoundary {
  readonly default: () => Promise<unknown>
  readonly create_sample: (request: unknown) => ApiResponse<OperationResultDto>
  readonly apply_command: (request: unknown) => ApiResponse<OperationResultDto>
  readonly stage_asset?: (
    bytes: Uint8Array,
    request: unknown,
  ) => ApiResponse<AssetStageResponseDto>
  readonly apply_editor_session?: (request: unknown) => ApiResponse<EditorSessionResponseDto>
  readonly open_document: (request: unknown) => ApiResponse<MigrateDocumentResultDto>
  readonly commit_record: (request: unknown) => ApiResponse<PlannedPersistenceCommitDto>
  readonly plan_standalone_audit: (request: unknown) => ApiResponse<AuditRecordDto>
  readonly query_document: (request: RecoveryRecordsDto) => ApiResponse<RecoverResultDto>
  readonly query_editor_view: (request: unknown) => ApiResponse<EditorViewDto>
  readonly recover_document_audited: (request: unknown) => ApiResponse<AuditedRecoverResultDto>
  readonly apply_form_session?: FormSessionWasmBoundary['apply_form_session']
  readonly project_form_widgets?: FormProjectionWasmBoundary['project_form_widgets']
  readonly verify_form_projection_response?:
    FormProjectionWasmBoundary['verify_form_projection_response']
  readonly layout_document?: (requestJson: string) => string
  readonly verify_layout_response?: (responseJson: string) => boolean
  readonly export_pdf?: (requestJson: string) => string
  readonly verify_pdf_export_response?: (responseJson: string) => boolean
  readonly recover_owned_source?: (requestJson: string) => string
  readonly font_catalog_identity?: (requestJson: string) => string
  readonly hyphenation_data_identity?: (requestJson: string) => string
  readonly resolve_voice_command?: (requestJson: string) => string
}

export interface EditorPersistence {
  commit(commit: PlannedPersistenceCommitDto): Promise<void>
  commitMigration(commit: MigrationPersistenceCommitDto): Promise<void>
  commitStandaloneAudit(auditRecord: AuditRecordDto): Promise<void>
  loadRecords(options?: { readonly allowEmpty?: boolean }): Promise<RecoveryRecordsDto>
  loadRecoveryImage(options?: { readonly allowEmpty?: boolean }): Promise<RecoveryImageDto>
  installRecoveredHead(records: RecoveryRecordsDto, head: DocumentHeadDto): Promise<void>
}

export interface EditorAppOptions {
  readonly databaseName?: string
  readonly locale?: EditorLocale
  readonly clock?: () => Date
}

export interface EditorControllerDependencies {
  readonly documentStore?: EditorPersistence
  readonly store?: EditorStore
  readonly wasm?: Promise<WasmBoundary>
  /** Optional background layout bridge; semantic editor state does not depend on its completion. */
  readonly layoutScheduler?: EditorLayoutScheduler
  readonly layoutRequestFactory?: (
    accepted: EditorAcceptedSnapshot,
  ) => LayoutRequestDto | null
  /** Optional background PDF export bridge; export bytes never enter editor state. */
  readonly pdfExportScheduler?: EditorPdfExportScheduler
  readonly pdfExportRequestFactory?: (
    accepted: EditorAcceptedSnapshot,
    layout: AcceptedLayoutDto,
    formSelection: FormProjectionSelectionDto | null,
  ) => PdfExportRequestDto | null
  /** Optional controlled PDF reader bridge; imported scenes remain read-only. */
  readonly pdfReaderScheduler?: EditorPdfReaderScheduler
  /** Optional external reconstruction bridge; candidates remain review-only until accepted. */
  readonly pdfReconstructionScheduler?: EditorPdfReconstructionScheduler
  /** Optional caller-owned source-bound noncanonical field session for voice. */
  readonly voiceFormSession?: VoiceFormSessionTarget
}

export interface EditorLayoutScheduler {
  request(request: LayoutRequestDto): Promise<LayoutScheduleOutcome>
  cancel(requestId?: string): void
  accepted(): AcceptedLayoutDto | null
  snapshot(): LayoutSchedulerSnapshotDto
  subscribe?(listener: () => void): () => void
}

export interface EditorPdfExportScheduler {
  request(request: PdfExportRequestDto): Promise<PdfExportScheduleOutcome>
  cancel(requestId?: string): void
  accepted(): AcceptedPdfExportDto | null
  snapshot(): PdfExportSchedulerSnapshotDto
  subscribe?(listener: () => void): () => void
}

export interface EditorPdfReaderScheduler {
  request(request: PdfReaderRequestDto): Promise<PdfReaderScheduleOutcome>
  cancel(requestId?: string): void
  accepted(): AcceptedPdfReaderDto | null
  snapshot(): PdfReaderSchedulerSnapshotDto
  subscribe?(listener: () => void): () => void
}

export interface EditorPdfReconstructionScheduler {
  request(request: PdfReconstructionRequestDto): Promise<PdfReconstructionScheduleOutcome>
  accept(decisions: readonly PdfReviewDecisionDto[]): Promise<PdfReconstructionAcceptOutcome>
  cancel(requestId?: string): void
  accepted(): AcceptedPdfReconstructionDto | null
  acceptedCandidate(): PdfReconstructionAcceptedCandidateDto | null
  snapshot(): PdfReconstructionSchedulerSnapshotDto
  subscribe?(listener: () => void): () => void
}

export const editorCopy = {
  uk: {
    documentTitle: 'FlowPDF — Семантичний редактор',
    product: 'FlowPDF',
    localOnly: 'локальна перевірка',
    title: 'Семантичний редактор документа',
    description: 'Неперервний текстовий простір із Rust-owned станом і локальною стійкістю.',
    region: 'Редактор документа',
    emptyTitle: 'Документ ще не відкрито',
    emptyBody: 'Створіть тестовий документ або відкрийте документ старішої схеми.',
    create: 'Створити документ',
    openOlder: 'Відкрити та перенести v2',
    openLast: 'Відкрити локальний документ',
    replace: 'Застосувати заміну абзацу',
    undo: 'Скасувати',
    redo: 'Повторити',
    reload: 'Перезавантажити',
    recover: 'Перевірити відновлення',
    revision: 'Ревізія',
    hash: 'Хеш',
    durable: 'Стан перевірено після commit/re-query',
    loading: 'Завантаження Rust-ядра…',
    pending: 'Зміна виконується…',
    created: 'Документ створено і перевірено зі сховища.',
    edited: 'Абзац змінено і перевірено зі сховища.',
    structural: 'Структуру документа змінено і перевірено зі сховища.',
    formatting: 'Форматування змінено і перевірено зі сховища.',
    voiceDictated: 'Голосовий фрагмент додано одним комітом і перевірено зі сховища.',
    voiceCommand: 'Голосову команду виконано і перевірено зі сховища.',
    selection: 'Виділення оновлено в Rust-сесії.',
    undone: 'Зміну скасовано і перевірено зі сховища.',
    redone: 'Зміну повторено і перевірено зі сховища.',
    reloaded: 'Локальний стан перезавантажено.',
    recovered: 'Durable стан відновлено Rust-ядром.',
    migrated: 'Документ перенесено у схему v2 і перевірено.',
    diagnostics: 'Інспектор основи та діагностика',
    diagnosticsError: 'Діагностику не вдалося відкрити.',
    error: 'Дію не виконано. Прийнятий стан не змінено. Код',
  },
  en: {
    documentTitle: 'FlowPDF — Semantic editor',
    product: 'FlowPDF',
    localOnly: 'local verification',
    title: 'Semantic document editor',
    description: 'A continuous text surface with Rust-owned state and local durability.',
    region: 'Document editor',
    emptyTitle: 'No document is open',
    emptyBody: 'Create a sample document or open the older-schema v2 fixture.',
    create: 'Create document',
    openOlder: 'Open and migrate to v2',
    openLast: 'Open local document',
    replace: 'Apply paragraph replacement',
    undo: 'Undo',
    redo: 'Redo',
    reload: 'Reload',
    recover: 'Verify recovery',
    revision: 'Revision',
    hash: 'Hash',
    durable: 'State verified after commit/re-query',
    loading: 'Loading the Rust core…',
    pending: 'Applying change…',
    created: 'Document created and verified from storage.',
    edited: 'Paragraph changed and verified from storage.',
    structural: 'Document structure changed and verified from storage.',
    formatting: 'Formatting changed and verified from storage.',
    voiceDictated: 'The dictated fragment was added in one commit and verified from storage.',
    voiceCommand: 'The voice command was executed and verified from storage.',
    selection: 'Selection updated in the Rust session.',
    undone: 'Change undone and verified from storage.',
    redone: 'Change redone and verified from storage.',
    reloaded: 'Local state reloaded.',
    recovered: 'Durable state recovered by the Rust core.',
    migrated: 'Document migrated to schema v2 and verified.',
    diagnostics: 'Foundation Inspector diagnostics',
    diagnosticsError: 'Diagnostics could not be opened.',
    error: 'The action was not completed. Accepted state was unchanged. Code',
  },
} as const

export function copy(locale: EditorLocale): (typeof editorCopy)[EditorLocale] {
  return editorCopy[locale]
}

const GENERATED_WASM_MODULE = '../../generated/flow_wasm.js'
const EMPTY_ASSET_HASH =
  'blake3:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262'
const EDITED_TEXT = ' — зміна / edit'
let wasmPromise: Promise<WasmBoundary> | undefined

export async function loadWasm(): Promise<WasmBoundary> {
  if (wasmPromise === undefined) {
    const pending = import(/* @vite-ignore */ GENERATED_WASM_MODULE).then(
      async (module: unknown) => {
        const boundary = module as WasmBoundary
        await boundary.default()
        return boundary
      },
    )
    wasmPromise = pending
    void pending.catch(() => {
      if (wasmPromise === pending) wasmPromise = undefined
    })
  }
  return wasmPromise
}

export class EditorController {
  private readonly documentStore: EditorPersistence
  private readonly locale: EditorLocale
  private readonly clock: () => Date
  private readonly wasm: Promise<WasmBoundary>
  private readonly stateStore: EditorStore
  private readonly layoutScheduler: EditorLayoutScheduler | undefined
  private readonly layoutRequestFactory:
    | ((accepted: EditorAcceptedSnapshot) => LayoutRequestDto | null)
    | undefined
  private readonly layoutUnsubscribe: (() => void) | undefined
  private readonly pdfExportScheduler: EditorPdfExportScheduler | undefined
  private readonly pdfExportRequestFactory:
    | ((
        accepted: EditorAcceptedSnapshot,
        layout: AcceptedLayoutDto,
        formSelection: FormProjectionSelectionDto | null,
      ) => PdfExportRequestDto | null)
    | undefined
  private voiceFormSession: VoiceFormSessionTarget | undefined
  private readonly pdfExportUnsubscribe: (() => void) | undefined
  private pdfRequestNumber = 0
  private initialized: Promise<void> | undefined
  private pending: Promise<void> = Promise.resolve()
  private sessionPending: Promise<void> = Promise.resolve()
  private readonly assetSessionId = newCommandId()
  private readonly imageObjectUrls = new Map<string, string>()

  constructor(
    options: EditorAppOptions = {},
    dependencies: EditorControllerDependencies = {},
  ) {
    this.locale = options.locale ?? 'uk'
    this.clock = options.clock ?? (() => new Date())
    this.documentStore =
      dependencies.documentStore ?? new IndexedDbDocumentStore(options.databaseName)
    this.stateStore = dependencies.store ?? new EditorStore(copy(this.locale).loading)
    this.wasm = dependencies.wasm ?? loadWasm()
    this.layoutScheduler = dependencies.layoutScheduler
    this.layoutRequestFactory = dependencies.layoutRequestFactory
    this.layoutUnsubscribe = this.layoutScheduler?.subscribe?.(() => {
      this.stateStore.publishLayout(this.layoutScheduler?.snapshot() ?? emptyLayoutSnapshot())
    })
    if (this.layoutScheduler !== undefined) {
      this.stateStore.publishLayout(this.layoutScheduler.snapshot())
    }
    this.pdfExportScheduler = dependencies.pdfExportScheduler
    this.pdfExportRequestFactory =
      dependencies.pdfExportRequestFactory ??
      (this.pdfExportScheduler === undefined
        ? undefined
        : (accepted, layout, formSelection) =>
            createPdfExportRequest(accepted, layout, {
              requestId: this.nextPdfRequestId(),
              formSelection,
            }))
    this.voiceFormSession = dependencies.voiceFormSession
    this.pdfExportUnsubscribe = this.pdfExportScheduler?.subscribe?.(() => {
      this.stateStore.publishPdf(
        this.pdfExportScheduler?.snapshot() ?? emptyPdfSnapshot(),
      )
    })
    if (this.pdfExportScheduler !== undefined) {
      this.stateStore.publishPdf(this.pdfExportScheduler.snapshot())
    }
  }

  readonly subscribe = (listener: () => void): (() => void) =>
    this.stateStore.subscribe(listener)

  readonly getSnapshot = () => this.stateStore.getSnapshot()

  snapshot() {
    return this.stateStore.getSnapshot()
  }

  /** Installs the caller-owned form session without replacing an explicit seam. */
  setVoiceFormSession(target: VoiceFormSessionTarget): void {
    this.voiceFormSession ??= target
  }

  /** Returns the last complete layout projection, if background layout is configured. */
  layoutSnapshot(): AcceptedLayoutDto | null {
    return this.layoutScheduler?.accepted() ?? null
  }

  /** Subscribes to complete layout publications without coupling editor input to the worker. */
  readonly subscribeLayout = (listener: () => void): (() => void) =>
    this.layoutScheduler?.subscribe?.(listener) ?? (() => undefined)

  /** Requests layout for the current accepted revision and returns immediately to the caller. */
  requestLayout(): Promise<LayoutScheduleOutcome> {
    const accepted = this.requireAccepted()
    if (this.layoutScheduler === undefined || this.layoutRequestFactory === undefined) {
      return Promise.resolve({
        kind: 'failed',
        requestId: 'layout-unavailable',
        code: 'FLOW_LAYOUT_UNAVAILABLE',
      })
    }
    const request = this.layoutRequestFactory(accepted)
    if (request === null) {
      return Promise.resolve({
        kind: 'failed',
        requestId: 'layout-unavailable',
        code: 'FLOW_LAYOUT_REQUEST_UNAVAILABLE',
      })
    }
    if (
      request.sourceRevision !== accepted.session.revision ||
      request.sourceHash !== accepted.session.canonicalHash
    ) {
      return Promise.resolve({
        kind: 'failed',
        requestId: request.requestId,
        code: 'FLOW_LAYOUT_SOURCE_REVISION_MISMATCH',
      })
    }
    return this.layoutScheduler.request(request)
  }

  cancelLayout(requestId?: string): void {
    this.layoutScheduler?.cancel(requestId)
  }

  /** Returns the last complete export for the accepted source/layout identity. */
  pdfExportSnapshot(): AcceptedPdfExportDto | null {
    return this.pdfExportScheduler?.accepted() ?? null
  }

  /** Whether the PDF preview/export adapter is wired for this controller. */
  hasPdfExport(): boolean {
    return this.pdfExportScheduler !== undefined && this.pdfExportRequestFactory !== undefined
  }

  requestPdfExport(
    formSelection: FormProjectionSelectionDto | null = null,
  ): Promise<PdfExportScheduleOutcome> {
    const accepted = this.requireAccepted()
    const layout = this.layoutScheduler?.accepted() ?? null
    if (this.pdfExportScheduler === undefined || this.pdfExportRequestFactory === undefined) {
      return Promise.resolve({
        kind: 'failed',
        requestId: 'pdf-export-unavailable',
        code: 'FLOW_PDF_EXPORT_UNAVAILABLE',
      })
    }
    if (layout === null) {
      return Promise.resolve({
        kind: 'failed',
        requestId: 'pdf-export-layout-unavailable',
        code: 'FLOW_PDF_LAYOUT_UNAVAILABLE',
      })
    }
    if (formSelection !== null) {
      const selectionError = validateFormProjectionSelection(formSelection)
      if (selectionError !== null) {
        return Promise.resolve({
          kind: 'failed',
          requestId: 'pdf-export-selection-invalid',
          code: selectionError,
        })
      }
      if (
        formSelection.sourceRevision !== accepted.session.revision ||
        formSelection.sourceHash !== accepted.session.canonicalHash
      ) {
        return Promise.resolve({
          kind: 'failed',
          requestId: 'pdf-export-selection-stale',
          code: 'FLOW_PDF_FORM_SELECTION_SOURCE_MISMATCH',
        })
      }
    }
    const request = this.pdfExportRequestFactory(accepted, layout, formSelection)
    if (request === null) {
      return Promise.resolve({
        kind: 'failed',
        requestId: 'pdf-export-request-unavailable',
        code: 'FLOW_PDF_EXPORT_REQUEST_UNAVAILABLE',
      })
    }
    if (
      request.sourceRevision !== accepted.session.revision ||
      request.sourceHash !== accepted.session.canonicalHash
    ) {
      return Promise.resolve({
        kind: 'failed',
        requestId: request.requestId,
        code: 'FLOW_PDF_SOURCE_REVISION_MISMATCH',
      })
    }
    if (request.layoutResultHash !== layout.result.resultHash) {
      return Promise.resolve({
        kind: 'failed',
        requestId: request.requestId,
        code: 'FLOW_PDF_LAYOUT_RESULT_MISMATCH',
      })
    }
    return this.pdfExportScheduler.request(request)
  }

  cancelPdfExport(requestId?: string): void {
    this.pdfExportScheduler?.cancel(requestId)
  }

  private nextPdfRequestId(): string {
    this.pdfRequestNumber += 1
    return `editor-pdf-${this.pdfRequestNumber}`
  }

  async initialize(): Promise<void> {
    this.initialized ??= this.initializeFromStorage()
    await this.initialized
  }

  whenIdle(): Promise<void> {
    return Promise.all([this.pending, this.sessionPending]).then(() => undefined)
  }

  createSample(): Promise<void> {
    return this.enqueue(async () => {
      const wasm = await this.wasm
      const result = unwrap(
        wasm.create_sample({
          requestedLocale: this.locale === 'uk' ? 'uk-UA' : 'en-US',
          issuedAt: this.currentTimestamp(),
        }),
      )
      await this.persistPlanned(result.commit, 'creation')
      await this.publishVerified(
        result.session,
        copy(this.locale).created,
        result.editor.session,
      )
    })
  }

  openOlderSchema(): Promise<void> {
    return this.enqueue(async () => {
      const wasm = await this.wasm
      const result = unwrap(
        wasm.open_document({
          fixture: 'supportedOlder',
          migrationId: newCommandId(),
          issuedAt: this.currentTimestamp(),
          assets: [
            {
              recordFormatVersion: 1,
              contentHash: EMPTY_ASSET_HASH,
              bytes: [],
            },
          ],
        }),
      )
      if (result.commit === null) throw new EditorError('FLOW_MIGRATION_BOUNDARY_REQUIRED')
      await this.documentStore.commitMigration(result.commit)
      await this.publishVerified(
        { revision: result.revisionProvenance.revision, canonicalHash: result.canonicalHash },
        copy(this.locale).migrated,
        result.editor.session,
      )
    })
  }

  replaceSelection(): Promise<void> {
    return this.replaceText(EDITED_TEXT, undefined, 'ui')
  }

  replaceText(
    text: string,
    selection?: DirectionalSelectionDto,
    modality: SourceModality = 'keyboard',
  ): Promise<void> {
    return this.enqueue(async () => {
      const accepted = this.requireAccepted()
      const request: ApplyCommandRequestDto = {
        canonicalJson: accepted.session.canonicalJson,
        history: accepted.session.history,
        command: {
          commandId: newCommandId(),
          baseRevision: accepted.session.revision,
          modality,
          issuedAt: this.currentTimestamp(),
          kind: {
            type: 'replaceSelection',
            selection: selection ?? accepted.editor.session.selection,
            text,
          },
        },
      }
      const wasm = await this.wasm
      const result = unwrap(wasm.apply_command(request))
      await this.persistPlanned(result.commit, 'committedTransaction')
      await this.publishVerified(
        result.session,
        copy(this.locale).edited,
        result.editor.session,
      )
    })
  }

  createVoiceDictationCapture(
    text: string,
    selection?: DirectionalSelectionDto,
  ): VoiceDictationCaptureDto {
    return createVoiceDictationCapture(
      this.requireAccepted(),
      text,
      selection,
    )
  }

  async resolveVoiceCommand(
    transcript: string,
    selection?: DirectionalSelectionDto,
    activeFieldId?: string,
  ): Promise<AcceptedVoiceIntentDto> {
    const accepted = this.requireAccepted()
    const capture = createVoiceCommandCapture(
      accepted,
      this.locale === 'uk' ? 'uk-UA' : 'en-US',
      transcript,
      selection,
      activeFieldId,
    )
    const wasm = await this.wasm
    if (wasm.resolve_voice_command === undefined) {
      throw new EditorError('FLOW_VOICE_UNAVAILABLE')
    }
    try {
      return resolveVoiceIntent(
        { resolve_voice_command: wasm.resolve_voice_command },
        capture,
      )
    } catch (error: unknown) {
      this.publishError(error)
      throw error
    }
  }

  dictateVoice(
    text: string,
    selection?: DirectionalSelectionDto,
  ): Promise<VoiceDispatchOutcome> {
    return this.dispatchVoiceDictation(this.createVoiceDictationCapture(text, selection))
  }

  dispatchVoiceDictation(
    capture: VoiceDictationCaptureDto,
  ): Promise<VoiceDispatchOutcome> {
    const validationError = validateVoiceDictationCapture(capture)
    if (validationError !== null) return this.rejectVoice(validationError)
    return this.enqueueVoice(async () => {
      const accepted = this.requireAccepted()
      this.assertVoiceSource(accepted, capture)
      const session = await this.applyVoiceCommand(
        {
          type: 'replaceSelection',
          selection: capture.selection,
          text: capture.text,
        },
        copy(this.locale).voiceDictated,
      )
      return this.voiceCommitted(session)
    })
  }

  dispatchVoiceCommand(intent: AcceptedVoiceIntentDto): Promise<VoiceDispatchOutcome> {
    const validationError = validateAcceptedVoiceIntent(intent)
    if (validationError !== null) return this.rejectVoice(validationError)
    return this.enqueueVoice(async () => {
      const accepted = this.requireAccepted()
      if (intent.selection === null) {
        throw new EditorError('FLOW_VOICE_SELECTION_REQUIRED')
      }
      this.assertVoiceSource(accepted, {
        sourceRevision: intent.sourceRevision,
        sourceHash: intent.sourceHash,
        selection: intent.selection,
      })

      let kind: ApplyCommandRequestDto['command']['kind'] | undefined
      switch (intent.action.type) {
        case 'undo':
          kind = { type: 'undo' }
          break
        case 'redo':
          kind = { type: 'redo' }
          break
        case 'deleteSelection':
          kind = { type: 'replaceSelection', selection: intent.selection, text: '' }
          break
        case 'setInlineMark':
          kind = {
            type: 'setInlineMark',
            selection: intent.selection,
            mark: intent.action.mark,
          }
          break
        case 'insertPageBreak': {
          const placement = accepted.editor.view.capabilities.find(
            (capability) => capability.name === 'insertPageBreak',
          )?.placement
          if (placement === undefined || placement === null) {
            throw new EditorError('FLOW_VOICE_INSERT_PLACEMENT_UNAVAILABLE')
          }
          kind = { type: 'insertPageBreak', placement }
          break
        }
        case 'removePageBreak':
          kind = { type: 'removePageBreak', pageBreakId: intent.action.pageBreakId }
          break
        case 'navigateField': {
          const fieldId = this.resolveVoiceFieldId(accepted, intent.activeFieldId, intent.action.direction)
          const field = accepted.editor.view.document.fields.find(
            (candidate) => candidate.descriptor.id === fieldId,
          )
          if (field === undefined) throw new EditorError('FLOW_VOICE_FIELD_NOT_FOUND')
          const point = field.descriptor.anchor.original
          const result = await this.applyVoiceEditorSession({
            type: 'setSelection',
            selection: { anchor: point, focus: point },
          })
          return this.voiceSessionCommitted(accepted, result.session.sessionGeneration)
        }
        case 'clearField':
          return this.clearVoiceField(accepted, intent.action.fieldId)
      }
      if (kind === undefined) throw new EditorError('FLOW_VOICE_ACTION_INVALID')
      const session = await this.applyVoiceCommand(kind, copy(this.locale).voiceCommand)
      return this.voiceCommitted(session)
    })
  }

  structuralCommand(
    kind: StructuralCommandDto,
    modality: SourceModality = 'keyboard',
  ): Promise<void> {
    return this.enqueue(async () => {
      const accepted = this.requireAccepted()
      const request: ApplyCommandRequestDto = {
        canonicalJson: accepted.session.canonicalJson,
        history: accepted.session.history,
        command: {
          commandId: newCommandId(),
          baseRevision: accepted.session.revision,
          modality,
          issuedAt: this.currentTimestamp(),
          kind,
        },
      }
      const wasm = await this.wasm
      const result = unwrap(wasm.apply_command(request))
      await this.persistPlanned(result.commit, 'committedTransaction')
      await this.publishVerified(
        result.session,
        copy(this.locale).structural,
        result.editor.session,
      )
    })
  }

  insertImage(
    bytes: Uint8Array,
    placement: StructuralPlacementDto,
    accessibility: Exclude<ImageAccessibilityDto, { readonly kind: 'missingLegacy' }>,
    modality: SourceModality = 'ui',
  ): Promise<void> {
    return this.enqueue(async () => {
      const accepted = this.requireAccepted()
      const wasm = await this.wasm
      const staged = this.stageAsset(wasm, bytes, accepted)
      const result = unwrap(
        wasm.apply_command({
          canonicalJson: accepted.session.canonicalJson,
          history: accepted.session.history,
          command: {
            commandId: newCommandId(),
            baseRevision: accepted.session.revision,
            modality,
            issuedAt: this.currentTimestamp(),
            kind: {
              type: 'insertImage',
              placement,
              sessionId: this.assetSessionId,
              receipt: staged.receipt,
              accessibility,
            },
          },
        }),
      )
      await this.persistPlanned(result.commit, 'committedTransaction')
      await this.publishVerified(
        result.session,
        copy(this.locale).structural,
        result.editor.session,
      )
    })
  }

  replaceImage(
    imageNodeId: string,
    bytes: Uint8Array,
    accessibility: Exclude<ImageAccessibilityDto, { readonly kind: 'missingLegacy' }>,
    modality: SourceModality = 'ui',
  ): Promise<void> {
    return this.enqueue(async () => {
      const accepted = this.requireAccepted()
      const wasm = await this.wasm
      const staged = this.stageAsset(wasm, bytes, accepted)
      const result = unwrap(
        wasm.apply_command({
          canonicalJson: accepted.session.canonicalJson,
          history: accepted.session.history,
          command: {
            commandId: newCommandId(),
            baseRevision: accepted.session.revision,
            modality,
            issuedAt: this.currentTimestamp(),
            kind: {
              type: 'replaceImage',
              imageNodeId,
              sessionId: this.assetSessionId,
              receipt: staged.receipt,
              accessibility,
            },
          },
        }),
      )
      await this.persistPlanned(result.commit, 'committedTransaction')
      await this.publishVerified(
        result.session,
        copy(this.locale).structural,
        result.editor.session,
      )
    })
  }

  setImageAccessibility(
    imageNodeId: string,
    accessibility: Exclude<ImageAccessibilityDto, { readonly kind: 'missingLegacy' }>,
    modality: SourceModality = 'ui',
  ): Promise<void> {
    return this.structuralCommand(
      { type: 'setImageAccessibility', imageNodeId, accessibility },
      modality,
    )
  }

  removeImage(
    imageNodeId: string,
    confirmed: boolean,
    modality: SourceModality = 'ui',
  ): Promise<void> {
    return this.structuralCommand(
      { type: 'removeImage', imageNodeId, confirmed },
      modality,
    )
  }

  imageSource(contentHash: string): string | null {
    return this.imageObjectUrls.get(contentHash) ?? null
  }

  splitTextBlock(
    nodeId: string,
    utf16Offset: number,
    newNodeId: string,
    modality: SourceModality = 'ui',
  ): Promise<void> {
    return this.structuralCommand(
      { type: 'splitTextBlock', nodeId, utf16Offset, newNodeId },
      modality,
    )
  }

  mergeTextBlocks(
    firstNodeId: string,
    secondNodeId: string,
    modality: SourceModality = 'ui',
  ): Promise<void> {
    return this.structuralCommand(
      { type: 'mergeTextBlocks', firstNodeId, secondNodeId },
      modality,
    )
  }

  deleteSubtree(nodeId: string, modality: SourceModality = 'ui'): Promise<void> {
    return this.structuralCommand({ type: 'deleteSubtree', nodeId }, modality)
  }

  formattingCommand(
    kind: FormattingCommandDto,
    modality: SourceModality = 'ui',
  ): Promise<void> {
    return this.enqueue(async () => {
      const accepted = this.requireAccepted()
      const request: ApplyCommandRequestDto = {
        canonicalJson: accepted.session.canonicalJson,
        history: accepted.session.history,
        command: {
          commandId: newCommandId(),
          baseRevision: accepted.session.revision,
          modality,
          issuedAt: this.currentTimestamp(),
          kind,
        },
      }
      const wasm = await this.wasm
      const result = unwrap(wasm.apply_command(request))
      await this.persistPlanned(result.commit, 'committedTransaction')
      await this.publishVerified(
        result.session,
        copy(this.locale).formatting,
        result.editor.session,
      )
    })
  }

  setInlineMark(
    mark: InlineMarkDto,
    selection?: DirectionalSelectionDto,
    modality: SourceModality = 'ui',
  ): Promise<void> {
    const accepted = this.requireAccepted()
    const target = selection ?? accepted.editor.session.selection
    if (target.anchor.nodeId === target.focus.nodeId &&
      target.anchor.utf16Offset === target.focus.utf16Offset) {
      return this.setPendingMark(mark)
    }
    return this.formattingCommand(
      { type: 'setInlineMark', selection: target, mark },
      modality,
    )
  }

  setInlineMarks(
    marks: MarkSetDto,
    selection?: DirectionalSelectionDto,
    modality: SourceModality = 'ui',
  ): Promise<void> {
    const accepted = this.requireAccepted()
    const target = selection ?? accepted.editor.session.selection
    if (target.anchor.nodeId === target.focus.nodeId &&
      target.anchor.utf16Offset === target.focus.utf16Offset) {
      return this.applyEditorSession({ type: 'setPendingMarks', marks })
    }
    return this.formattingCommand(
      { type: 'setInlineMarks', selection: target, marks },
      modality,
    )
  }

  setBlockStyle(
    style: BlockStyleDto,
    selection?: DirectionalSelectionDto,
    modality: SourceModality = 'ui',
  ): Promise<void> {
    return this.formattingCommand(
      {
        type: 'setBlockStyle',
        selection: selection ?? this.requireAccepted().editor.session.selection,
        style,
      },
      modality,
    )
  }

  setBlockAttributes(
    attributes: BlockAttributesDto,
    selection?: DirectionalSelectionDto,
    modality: SourceModality = 'ui',
  ): Promise<void> {
    return this.formattingCommand(
      {
        type: 'setBlockAttributes',
        selection: selection ?? this.requireAccepted().editor.session.selection,
        attributes,
      },
      modality,
    )
  }

  setListKind(
    kind: ListKindDto,
    selection?: DirectionalSelectionDto,
    modality: SourceModality = 'ui',
  ): Promise<void> {
    return this.formattingCommand(
      {
        type: 'setListKind',
        selection: selection ?? this.requireAccepted().editor.session.selection,
        kind,
      },
      modality,
    )
  }

  continueListItem(
    selection?: DirectionalSelectionDto,
    modality: SourceModality = 'keyboard',
  ): Promise<void> {
    return this.formattingCommand(
      {
        type: 'continueListItem',
        selection: selection ?? this.requireAccepted().editor.session.selection,
      },
      modality,
    )
  }

  exitListItem(
    selection?: DirectionalSelectionDto,
    modality: SourceModality = 'keyboard',
  ): Promise<void> {
    return this.formattingCommand(
      {
        type: 'exitListItem',
        selection: selection ?? this.requireAccepted().editor.session.selection,
      },
      modality,
    )
  }

  indentListItem(itemId: string, modality: SourceModality = 'ui'): Promise<void> {
    return this.formattingCommand({ type: 'indentListItem', itemId }, modality)
  }

  outdentListItem(itemId: string, modality: SourceModality = 'ui'): Promise<void> {
    return this.formattingCommand({ type: 'outdentListItem', itemId }, modality)
  }

  setEditorSelection(selection: DirectionalSelectionDto): Promise<void> {
    const accepted = this.stateStore.accepted()
    if (accepted === null || sameSelection(accepted.editor.session.selection, selection)) {
      return Promise.resolve()
    }
    return this.applyEditorSession({ type: 'setSelection', selection })
  }

  setPendingMark(mark: InlineMarkDto): Promise<void> {
    return this.applyEditorSession({ type: 'setPendingMark', mark })
  }

  undo(): Promise<void> {
    return this.applyHistory('undo', copy(this.locale).undone)
  }

  redo(): Promise<void> {
    return this.applyHistory('redo', copy(this.locale).redone)
  }

  reloadFromStorage(): Promise<void> {
    return this.enqueue(async () => {
      await this.restoreFromStorage(copy(this.locale).reloaded)
    })
  }

  recoverFromStorage(): Promise<void> {
    return this.enqueue(async () => {
      await this.restoreFromStorage(copy(this.locale).recovered)
    })
  }

  dispose(): void {
    this.layoutScheduler?.cancel()
    this.layoutUnsubscribe?.()
    this.pdfExportScheduler?.cancel()
    this.pdfExportUnsubscribe?.()
    if (typeof globalThis.URL?.revokeObjectURL === 'function') {
      for (const objectUrl of this.imageObjectUrls.values()) {
        globalThis.URL.revokeObjectURL(objectUrl)
      }
    }
    this.imageObjectUrls.clear()
    this.stateStore.dispose()
  }

  private async initializeFromStorage(): Promise<void> {
    try {
      const records = await this.documentStore.loadRecords({ allowEmpty: true })
      if (records.snapshots.length === 0 && records.transactions.length === 0) {
        this.stateStore.publishEmpty()
        return
      }
      const wasm = await this.wasm
      const recovered = unwrap(wasm.query_document(records))
      this.syncImagePreviews(records.assets, recovered.editor.view)
      this.publishAccepted(recovered, copy(this.locale).reloaded)
    } catch (error: unknown) {
      this.publishError(error)
    }
  }

  private applyHistory(kind: 'undo' | 'redo', status: string): Promise<void> {
    return this.enqueue(async () => {
      const accepted = this.requireAccepted()
      const request: ApplyCommandRequestDto = {
        canonicalJson: accepted.session.canonicalJson,
        history: accepted.session.history,
        command: {
          commandId: newCommandId(),
          baseRevision: accepted.session.revision,
          modality: 'ui',
          issuedAt: this.currentTimestamp(),
          kind: { type: kind },
        },
      }
      const wasm = await this.wasm
      const result = unwrap(wasm.apply_command(request))
      await this.persistPlanned(result.commit, 'committedTransaction')
      await this.publishVerified(result.session, status, result.editor.session)
    })
  }

  private async applyVoiceCommand(
    kind: ApplyCommandRequestDto['command']['kind'],
    status: string,
  ): Promise<SessionDto> {
    const accepted = this.requireAccepted()
    const wasm = await this.wasm
    const result = unwrap(
      wasm.apply_command({
        canonicalJson: accepted.session.canonicalJson,
        history: accepted.session.history,
        command: {
          commandId: newCommandId(),
          baseRevision: accepted.session.revision,
          modality: 'voice',
          issuedAt: this.currentTimestamp(),
          kind,
        },
      }),
    )
    await this.persistPlanned(result.commit, 'committedTransaction')
    await this.publishVerified(result.session, status, result.editor.session)
    return result.session
  }

  private assertVoiceSource(
    accepted: EditorAcceptedSnapshot,
    source: Pick<VoiceDictationCaptureDto, 'sourceRevision' | 'sourceHash' | 'selection'>,
  ): void {
    if (
      source.sourceRevision !== accepted.session.revision ||
      source.sourceHash !== accepted.session.canonicalHash
    ) {
      throw new EditorError('FLOW_VOICE_SOURCE_STALE')
    }
    if (!sameSelection(accepted.editor.session.selection, source.selection)) {
      throw new EditorError('FLOW_VOICE_SELECTION_STALE')
    }
  }

  private voiceCommitted(session: SessionDto): VoiceDispatchOutcome {
    const outcome: VoiceDispatchOutcome = {
      kind: 'committed',
      revision: session.revision,
      canonicalHash: session.canonicalHash,
    }
    this.stateStore.publishVoice({
      phase: 'committed',
      revision: session.revision,
      errorCode: null,
    })
    return outcome
  }

  private voiceSessionCommitted(
    accepted: EditorAcceptedSnapshot,
    sessionGeneration: number,
  ): VoiceDispatchOutcome {
    const outcome: VoiceDispatchOutcome = {
      kind: 'sessionCommitted',
      revision: accepted.session.revision,
      canonicalHash: accepted.session.canonicalHash,
      sessionGeneration,
    }
    this.stateStore.publishVoice({
      phase: 'committed',
      revision: accepted.session.revision,
      errorCode: null,
    })
    return outcome
  }

  private resolveVoiceFieldId(
    accepted: EditorAcceptedSnapshot,
    activeFieldId: string | undefined,
    direction: 'next' | 'previous',
  ): string {
    if (
      activeFieldId !== undefined &&
      accepted.editor.view.document.fieldReview.some(
        (field) => field.descriptor.id === activeFieldId,
      )
    ) {
      throw new EditorError('FLOW_VOICE_FIELD_REVIEW_REQUIRED')
    }
    const resolution = resolveVoiceFieldTarget(
      accepted.editor.view.document.fields.map((field) => ({
        fieldId: field.descriptor.id,
        tabOrder: field.tabOrder,
      })),
      activeFieldId,
      direction,
    )
    if (resolution.kind === 'rejected') throw new EditorError(resolution.code)
    return resolution.fieldId
  }

  private async clearVoiceField(
    accepted: EditorAcceptedSnapshot,
    fieldId: string,
  ): Promise<VoiceDispatchOutcome> {
    const field = accepted.editor.view.document.fields.find(
      (candidate) => candidate.descriptor.id === fieldId,
    )
    if (field === undefined) {
      if (
        accepted.editor.view.document.fieldReview.some(
          (candidate) => candidate.descriptor.id === fieldId,
        )
      ) {
        throw new EditorError('FLOW_VOICE_FIELD_REVIEW_REQUIRED')
      }
      throw new EditorError('FLOW_VOICE_FIELD_NOT_FOUND')
    }
    if (field.descriptor.readOnly) throw new EditorError('FLOW_VOICE_FIELD_READ_ONLY')
    const formSession = this.voiceFormSession
    if (formSession === undefined) {
      throw new EditorError('FLOW_VOICE_FORM_SESSION_UNAVAILABLE')
    }
    const identity = formSessionIdentity(accepted)
    const before = formSession.getSnapshot()
    if (
      before.phase !== 'ready' ||
      before.session === null ||
      !sameFormSessionIdentity(before.identity, identity)
    ) {
      throw new EditorError('FLOW_VOICE_FORM_SESSION_NOT_READY')
    }
    await formSession.clearValue(accepted.session.canonicalJson, identity, fieldId)
    const after = formSession.getSnapshot()
    if (
      after.phase !== 'ready' ||
      after.session === null ||
      !sameFormSessionIdentity(after.identity, identity)
    ) {
      throw new EditorError('FLOW_VOICE_FORM_SESSION_SOURCE_STALE')
    }
    return this.voiceSessionCommitted(accepted, after.session.generation)
  }

  private rejectVoice(code: string): Promise<VoiceDispatchOutcome> {
    this.publishError(new EditorError(code))
    this.stateStore.publishVoice({ phase: 'error', revision: null, errorCode: code })
    return Promise.resolve({ kind: 'rejected', code })
  }

  private enqueueVoice(
    operation: () => Promise<VoiceDispatchOutcome>,
  ): Promise<VoiceDispatchOutcome> {
    const task = this.pending.then(async () => {
      this.stateStore.publishPending(copy(this.locale).pending)
      try {
        return await operation()
      } catch (error: unknown) {
        const code = errorCode(error)
        this.publishError(error)
        this.stateStore.publishVoice({ phase: 'error', revision: null, errorCode: code })
        return { kind: 'rejected', code } as const
      }
    })
    this.pending = task.then(
      () => undefined,
      () => undefined,
    )
    return task
  }

  private applyEditorSession(action: EditorSessionActionDto): Promise<void> {
    const commandBarrier = this.pending
    const task = this.sessionPending.then(() => commandBarrier).then(async () => {
      const accepted = this.requireAccepted()
      const wasm = await this.wasm
      if (wasm.apply_editor_session === undefined) {
        throw new EditorError('FLOW_EDITOR_SESSION_UNAVAILABLE')
      }
      const result = unwrap(
        wasm.apply_editor_session({
          canonicalJson: accepted.session.canonicalJson,
          session: accepted.editor.session,
          action,
        }),
      )
      this.publishEditorSession(result)
    })
    this.sessionPending = task.then(
      () => undefined,
      () => undefined,
    )
    return task
  }

  private async applyVoiceEditorSession(
    action: EditorSessionActionDto,
  ): Promise<EditorSessionResponseDto> {
    await this.sessionPending
    const accepted = this.requireAccepted()
    const wasm = await this.wasm
    if (wasm.apply_editor_session === undefined) {
      throw new EditorError('FLOW_EDITOR_SESSION_UNAVAILABLE')
    }
    const result = unwrap(
      wasm.apply_editor_session({
        canonicalJson: accepted.session.canonicalJson,
        session: accepted.editor.session,
        action,
      }),
    )
    this.publishEditorSession(result)
    return result
  }

  private enqueue(operation: () => Promise<void>): Promise<void> {
    const task = this.pending.then(async () => {
      this.stateStore.publishPending(copy(this.locale).pending)
      try {
        await operation()
      } catch (error: unknown) {
        this.publishError(error)
      }
    })
    this.pending = task.then(
      () => undefined,
      () => undefined,
    )
    return task
  }

  private async restoreFromStorage(status: string): Promise<void> {
    const recoveryImage = await this.documentStore.loadRecoveryImage({ allowEmpty: true })
    const records = recoveryImage.records
    if (records.snapshots.length === 0 && records.transactions.length === 0) {
      this.stateStore.publishEmpty()
      return
    }
    const wasm = await this.wasm
    const expected =
      this.stateStore.accepted()?.session ?? unwrap(wasm.query_document(records)).session
    const auditContext: RecoveryAuditContextDto = {
      attemptId: newCommandId(),
      documentId: expected.documentId,
      expectedRevision: expected.revision,
      issuedAt: this.currentTimestamp(),
    }
    const derivation: StandaloneAuditDerivationDto = { type: 'recovery', auditContext }
    const audited = unwrap(wasm.recover_document_audited({ records, auditContext }))
    const recovered = audited.recovered
    await this.documentStore.installRecoveredHead(records, {
      documentId: recovered.session.documentId,
      revision: recovered.session.revision,
      canonicalHash: recovered.session.canonicalHash,
    })
    await this.persistStandaloneAudit(wasm, records, derivation)
    await this.publishVerified(
      { revision: recovered.session.revision, canonicalHash: recovered.session.canonicalHash },
      status,
    )
  }

  private async publishVerified(
    expected: Pick<SessionDto, 'revision' | 'canonicalHash'>,
    status: string,
    preferredSession?: EditorSessionStateDto,
  ): Promise<void> {
    const wasm = await this.wasm
    const records = await this.documentStore.loadRecords()
    const recovered = unwrap(wasm.query_document(records))
    if (
      recovered.session.revision !== expected.revision ||
      recovered.session.canonicalHash !== expected.canonicalHash
    ) {
      throw new EditorError('FLOW_RESULT_REVISION_MISMATCH')
    }
    const editorView =
      preferredSession === undefined
        ? recovered.editor.view
        : unwrap(
            wasm.query_editor_view({
              canonicalJson: recovered.session.canonicalJson,
              session: preferredSession,
            }),
          )
    this.syncImagePreviews(records.assets, editorView)
    if (preferredSession === undefined) {
      this.publishAccepted(recovered, status)
      return
    }
    this.publishAccepted(
      {
        ...recovered,
        editor: { session: preferredSession, view: editorView },
      },
      status,
    )
  }

  private async persistPlanned(
    commit: PersistenceCommitDto,
    reason: 'creation' | 'committedTransaction',
  ): Promise<void> {
    const wasm = await this.wasm
    const records = await this.documentStore.loadRecords({ allowEmpty: true })
    const planned = unwrap(wasm.commit_record({ records, commit, reason }))
    await this.documentStore.commit(planned)
  }

  private async persistStandaloneAudit(
    wasm: WasmBoundary,
    records: RecoveryRecordsDto,
    derivation: StandaloneAuditDerivationDto,
  ): Promise<void> {
    const authorized = unwrap(wasm.plan_standalone_audit({ records, derivation }))
    await this.documentStore.commitStandaloneAudit(authorized)
  }

  private requireAccepted(): EditorAcceptedSnapshot {
    const accepted = this.stateStore.accepted()
    if (accepted === null) throw new EditorError('FLOW_NO_ACTIVE_DOCUMENT')
    return accepted
  }

  private stageAsset(
    wasm: WasmBoundary,
    bytes: Uint8Array,
    accepted: EditorAcceptedSnapshot,
  ): AssetStageResponseDto {
    if (wasm.stage_asset === undefined) {
      throw new EditorError('FLOW_ASSET_STAGING_UNAVAILABLE')
    }
    return unwrap(
      wasm.stage_asset(bytes, {
        sessionId: this.assetSessionId,
        documentId: accepted.session.documentId,
        revision: accepted.session.revision,
      }),
    )
  }

  private syncImagePreviews(
    assets: readonly AssetRecordDto[],
    view: EditorViewDto,
  ): void {
    if (typeof globalThis.URL?.createObjectURL !== 'function') return
    if (typeof Blob === 'undefined') return
    const images = collectImageBlocks(view.document.blocks)
    const reachable = new Set(images.map((image) => image.contentHash))
    const records = new Map(assets.map((asset) => [asset.contentHash, asset]))
    for (const image of images) {
      if (this.imageObjectUrls.has(image.contentHash)) continue
      const asset = records.get(image.contentHash)
      if (asset === undefined || asset.bytes.length === 0) continue
      try {
        const objectUrl = globalThis.URL.createObjectURL(
          new Blob([Uint8Array.from(asset.bytes)], { type: image.mediaType }),
        )
        this.imageObjectUrls.set(image.contentHash, objectUrl)
      } catch {
        // A preview failure must not reject an already durable Rust state.
      }
    }
    for (const [contentHash, objectUrl] of this.imageObjectUrls) {
      if (reachable.has(contentHash)) continue
      globalThis.URL.revokeObjectURL(objectUrl)
      this.imageObjectUrls.delete(contentHash)
    }
  }

  private currentTimestamp(): string {
    return this.clock().toISOString().replace(/\.\d{3}Z$/, 'Z')
  }

  private publishAccepted(result: RecoverResultDto, status: string): void {
    const previous = this.stateStore.accepted()
    const accepted = {
      session: result.session,
      editor: result.editor,
      view: result.view,
    }
    this.stateStore.publishAccepted(accepted, status)
    if (
      previous !== null &&
      (previous.session.revision !== accepted.session.revision ||
        previous.session.canonicalHash !== accepted.session.canonicalHash)
    ) {
      this.pdfExportScheduler?.cancel()
      this.stateStore.publishPdf(emptyPdfSnapshot())
    }
    this.scheduleLayout(accepted)
  }

  private publishEditorSession(result: EditorSessionResponseDto): void {
    const accepted = this.stateStore.accepted()
    if (accepted === null) throw new EditorError('FLOW_NO_ACTIVE_DOCUMENT')
    this.stateStore.publishAccepted(
      {
        ...accepted,
        editor: result,
      },
      this.snapshot().status,
    )
  }

  private publishError(error: unknown): void {
    this.stateStore.publishError(errorCode(error))
  }

  private scheduleLayout(accepted: EditorAcceptedSnapshot): void {
    if (this.layoutScheduler === undefined || this.layoutRequestFactory === undefined) return
    const request = this.layoutRequestFactory(accepted)
    if (
      request === null ||
      request.sourceRevision !== accepted.session.revision ||
      request.sourceHash !== accepted.session.canonicalHash
    ) {
      return
    }
    // Layout is a derived background projection. A slow worker must never
    // extend the command/session barrier or block active IME/editor input.
    void this.layoutScheduler.request(request)
  }
}

function emptyLayoutSnapshot(): LayoutSchedulerSnapshotDto {
  return {
    phase: 'idle',
    accepted: null,
    requestId: null,
    errorCode: null,
  }
}

function emptyPdfSnapshot(): PdfExportSchedulerSnapshotDto {
  return {
    phase: 'idle',
    accepted: null,
    requestId: null,
    errorCode: null,
  }
}

export class EditorError extends Error {
  constructor(readonly code: string) {
    super(code)
    this.name = 'EditorError'
  }
}

function unwrap<T>(response: ApiResponse<T>): T {
  if (!response.ok || response.value === null) {
    throw new EditorError(response.error?.code ?? 'FLOW_UNKNOWN_CORE_ERROR')
  }
  return response.value
}

function newCommandId(): string {
  return globalThis.crypto.randomUUID()
}

function errorCode(error: unknown): string {
  if (error instanceof EditorError || error instanceof StorageError) return error.code
  return 'FLOW_UNEXPECTED_ERROR'
}

function sameSelection(
  left: DirectionalSelectionDto,
  right: DirectionalSelectionDto,
): boolean {
  return (
    left.anchor.nodeId === right.anchor.nodeId &&
    left.anchor.utf16Offset === right.anchor.utf16Offset &&
    left.anchor.affinity === right.anchor.affinity &&
    left.focus.nodeId === right.focus.nodeId &&
    left.focus.utf16Offset === right.focus.utf16Offset &&
    left.focus.affinity === right.focus.affinity
  )
}

function formSessionIdentity(accepted: EditorAcceptedSnapshot): FormSessionIdentityDto {
  return {
    documentId: accepted.session.documentId,
    sourceRevision: accepted.session.revision,
    sourceHash: accepted.session.canonicalHash,
  }
}

function sameFormSessionIdentity(
  left: FormSessionIdentityDto | null,
  right: FormSessionIdentityDto,
): boolean {
  return (
    left !== null &&
    left.documentId === right.documentId &&
    left.sourceRevision === right.sourceRevision &&
    left.sourceHash === right.sourceHash
  )
}

function collectImageBlocks(
  blocks: readonly EditorBlockViewDto[],
): ImageBlockViewDto[] {
  const images: ImageBlockViewDto[] = []
  for (const block of blocks) {
    if (block.image !== undefined) images.push(block.image)
    images.push(...collectImageBlocks(block.children ?? []))
  }
  return images
}
