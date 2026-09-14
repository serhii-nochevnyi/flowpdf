import {
  IndexedDbDocumentStore,
  StorageError,
  type AuditRecordDto,
  type DocumentHeadDto,
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
  type EditorLocale,
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
} from './editor-store.js'

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
  readonly apply_editor_session?: (request: unknown) => ApiResponse<EditorSessionResponseDto>
  readonly open_document: (request: unknown) => ApiResponse<MigrateDocumentResultDto>
  readonly commit_record: (request: unknown) => ApiResponse<PlannedPersistenceCommitDto>
  readonly plan_standalone_audit: (request: unknown) => ApiResponse<AuditRecordDto>
  readonly query_document: (request: RecoveryRecordsDto) => ApiResponse<RecoverResultDto>
  readonly query_editor_view: (request: unknown) => ApiResponse<EditorViewDto>
  readonly recover_document_audited: (request: unknown) => ApiResponse<AuditedRecoverResultDto>
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

async function loadWasm(): Promise<WasmBoundary> {
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
  private initialized: Promise<void> | undefined
  private pending: Promise<void> = Promise.resolve()
  private sessionPending: Promise<void> = Promise.resolve()

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
  }

  readonly subscribe = (listener: () => void): (() => void) =>
    this.stateStore.subscribe(listener)

  readonly getSnapshot = () => this.stateStore.getSnapshot()

  snapshot() {
    return this.stateStore.getSnapshot()
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
    if (preferredSession === undefined) {
      this.publishAccepted(recovered, status)
      return
    }
    const editorView = unwrap(
      wasm.query_editor_view({
        canonicalJson: recovered.session.canonicalJson,
        session: preferredSession,
      }),
    )
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

  private currentTimestamp(): string {
    return this.clock().toISOString().replace(/\.\d{3}Z$/, 'Z')
  }

  private publishAccepted(result: RecoverResultDto, status: string): void {
    this.stateStore.publishAccepted(
      {
        session: result.session,
        editor: result.editor,
        view: result.view,
      },
      status,
    )
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
