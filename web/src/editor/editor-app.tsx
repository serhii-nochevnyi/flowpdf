import { createRoot } from 'react-dom/client'
import { useEffect, useRef, useState, useSyncExternalStore } from 'react'

import {
  IndexedDbDocumentStore,
  StorageError,
  type AuditRecordDto,
  type HistoryStateDto,
  type LogicalPositionDto,
  type MigrationPersistenceCommitDto,
  type PersistenceCommitDto,
  type PlannedPersistenceCommitDto,
  type RecoveryRecordsDto,
} from '../../persistence/indexeddb-store.js'
import {
  mountFoundationInspector,
  type FoundationInspectorLocale,
} from '../foundation-inspector.js'
import { SemanticDocument } from './semantic-document.js'

export type EditorLocale = FoundationInspectorLocale

export interface DirectionalSelectionDto {
  readonly anchor: LogicalPositionDto
  readonly focus: LogicalPositionDto
}

export interface EditorSessionStateDto {
  readonly documentId: string
  readonly revision: number
  readonly sessionGeneration: number
  readonly selection: DirectionalSelectionDto
  readonly pendingMarks: unknown
  readonly formatting: unknown
  readonly capabilities: readonly unknown[]
}

export interface EditorBlockViewDto {
  readonly kind: 'paragraph' | 'heading' | 'atomic'
  readonly nodeId: string
  readonly text?: string
  readonly level?: number
  readonly nodeKind?: string
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
  readonly pendingMarks: unknown
  readonly formatting: unknown
  readonly capabilities: readonly unknown[]
  readonly document: EditorDocumentViewDto
}

export interface EditorSessionResponseDto {
  readonly session: EditorSessionStateDto
  readonly view: EditorViewDto
}

interface ErrorDto {
  readonly code: string
  readonly message: string
  readonly audit: AuditRecordDto | null
}

interface ApiResponse<T> {
  readonly ok: boolean
  readonly value: T | null
  readonly error: ErrorDto | null
}

interface SessionDto {
  readonly canonicalJson: string
  readonly canonicalHash: string
  readonly documentId: string
  readonly revision: number
  readonly nextCommandTarget: LogicalPositionDto | null
  readonly history: HistoryStateDto
}

interface RevisionProvenanceDto {
  readonly documentId: string
  readonly revision: number
  readonly schemaVersion: number
  readonly canonicalHash: string
  readonly lineage: unknown
  readonly engine: unknown
  readonly sourceHashes: readonly string[]
  readonly previewExportProvenance: string
}

interface InspectorViewDto {
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
  readonly revisionProvenance: RevisionProvenanceDto
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
    readonly modality: 'ui' | 'keyboard' | 'voice' | 'api' | 'system'
    readonly issuedAt: string
    readonly kind:
      | { readonly type: 'replaceSelection'; readonly selection: DirectionalSelectionDto; readonly text: string }
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

interface WasmBoundary {
  readonly default: () => Promise<unknown>
  readonly create_sample: (request: unknown) => ApiResponse<OperationResultDto>
  readonly apply_command: (request: unknown) => ApiResponse<OperationResultDto>
  readonly open_document: (request: unknown) => ApiResponse<MigrateDocumentResultDto>
  readonly commit_record: (request: unknown) => ApiResponse<PlannedPersistenceCommitDto>
  readonly plan_standalone_audit: (request: unknown) => ApiResponse<AuditRecordDto>
  readonly query_document: (request: RecoveryRecordsDto) => ApiResponse<RecoverResultDto>
  readonly query_editor_view: (request: unknown) => ApiResponse<EditorViewDto>
  readonly recover_document_audited: (request: unknown) => ApiResponse<AuditedRecoverResultDto>
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

export interface EditorAppOptions {
  readonly databaseName?: string
  readonly locale?: EditorLocale
  readonly clock?: () => Date
}

const GENERATED_WASM_MODULE = '../../generated/flow_wasm.js'
const EMPTY_ASSET_HASH =
  'blake3:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262'
const EDITED_TEXT = ' — зміна / edit'
let wasmPromise: Promise<WasmBoundary> | undefined

const editorCopy = {
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

function copy(locale: EditorLocale): (typeof editorCopy)[EditorLocale] {
  return editorCopy[locale]
}

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
  private readonly listeners = new Set<() => void>()
  private readonly store: IndexedDbDocumentStore
  private readonly locale: EditorLocale
  private readonly clock: () => Date
  private readonly wasm: Promise<WasmBoundary>
  private accepted: EditorAcceptedSnapshot | null = null
  private initialized: Promise<void> | undefined
  private pending: Promise<void> = Promise.resolve()
  private snapshotValue: EditorAppSnapshot

  constructor(options: EditorAppOptions = {}) {
    this.store = new IndexedDbDocumentStore(options.databaseName)
    this.locale = options.locale ?? 'uk'
    this.clock = options.clock ?? (() => new Date())
    this.wasm = loadWasm()
    this.snapshotValue = Object.freeze({
      phase: 'loading',
      accepted: null,
      status: copy(this.locale).loading,
      errorCode: null,
    })
  }

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  readonly getSnapshot = (): EditorAppSnapshot => this.snapshotValue

  snapshot(): EditorAppSnapshot {
    return this.snapshotValue
  }

  async initialize(): Promise<void> {
    this.initialized ??= this.initializeFromStorage()
    await this.initialized
  }

  whenIdle(): Promise<void> {
    return this.pending
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
      await this.store.commitMigration(result.commit)
      await this.publishVerified(
        { revision: result.revisionProvenance.revision, canonicalHash: result.canonicalHash },
        copy(this.locale).migrated,
        result.editor.session,
      )
    })
  }

  replaceSelection(): Promise<void> {
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
          kind: {
            type: 'replaceSelection',
            selection: accepted.editor.session.selection,
            text: EDITED_TEXT,
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
    this.listeners.clear()
  }

  private async initializeFromStorage(): Promise<void> {
    try {
      const records = await this.store.loadRecords({ allowEmpty: true })
      if (records.snapshots.length === 0 && records.transactions.length === 0) {
        this.publishEmpty()
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

  private enqueue(operation: () => Promise<void>): Promise<void> {
    const task = this.pending.then(async () => {
      this.publishPending()
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
    const recoveryImage = await this.store.loadRecoveryImage({ allowEmpty: true })
    const records = recoveryImage.records
    if (records.snapshots.length === 0 && records.transactions.length === 0) {
      this.publishEmpty()
      return
    }
    const wasm = await this.wasm
    const expected =
      this.accepted?.session ?? unwrap(wasm.query_document(records)).session
    const auditContext: RecoveryAuditContextDto = {
      attemptId: newCommandId(),
      documentId: expected.documentId,
      expectedRevision: expected.revision,
      issuedAt: this.currentTimestamp(),
    }
    const derivation: StandaloneAuditDerivationDto = { type: 'recovery', auditContext }
    const audited = unwrap(
      wasm.recover_document_audited({ records, auditContext }),
    )
    const recovered = audited.recovered
    await this.store.installRecoveredHead(records, {
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
    const records = await this.store.loadRecords()
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
      { ...recovered, editor: { session: preferredSession, view: editorView } },
      status,
    )
  }

  private async persistPlanned(
    commit: PersistenceCommitDto,
    reason: 'creation' | 'committedTransaction',
  ): Promise<void> {
    const wasm = await this.wasm
    const records = await this.store.loadRecords({ allowEmpty: true })
    const planned = unwrap(
      wasm.commit_record({ records, commit, reason }),
    )
    await this.store.commit(planned)
  }

  private async persistStandaloneAudit(
    wasm: WasmBoundary,
    records: RecoveryRecordsDto,
    derivation: StandaloneAuditDerivationDto,
  ): Promise<void> {
    const authorized = unwrap(
      wasm.plan_standalone_audit({ records, derivation }),
    )
    await this.store.commitStandaloneAudit(authorized)
  }

  private requireAccepted(): EditorAcceptedSnapshot {
    if (this.accepted === null) throw new EditorError('FLOW_NO_ACTIVE_DOCUMENT')
    return this.accepted
  }

  private currentTimestamp(): string {
    return this.clock().toISOString().replace(/\.\d{3}Z$/, 'Z')
  }

  private publishPending(): void {
    this.publishSnapshot({
      phase: 'pending',
      accepted: this.accepted,
      status: copy(this.locale).pending,
      errorCode: null,
    })
  }

  private publishEmpty(): void {
    this.accepted = null
    this.publishSnapshot({
      phase: 'empty',
      accepted: null,
      status: '',
      errorCode: null,
    })
  }

  private publishAccepted(result: RecoverResultDto, status: string): void {
    const accepted = Object.freeze({
      session: result.session,
      editor: result.editor,
      view: result.view,
    })
    this.accepted = accepted
    this.publishSnapshot({
      phase: 'ready',
      accepted,
      status,
      errorCode: null,
    })
  }

  private publishError(error: unknown): void {
    const code = errorCode(error)
    this.publishSnapshot({
      phase: 'error',
      accepted: this.accepted,
      status: '',
      errorCode: code,
    })
  }

  private publishSnapshot(snapshot: EditorAppSnapshot): void {
    this.snapshotValue = Object.freeze(snapshot)
    for (const listener of this.listeners) listener()
  }
}

export interface EditorAppProps {
  readonly controller?: EditorController
  readonly options?: EditorAppOptions
}

export function EditorApp({ controller: suppliedController, options = {} }: EditorAppProps) {
  const [controller] = useState(
    () => suppliedController ?? new EditorController(options),
  )
  const snapshot = useSyncExternalStore(
    controller.subscribe,
    controller.getSnapshot,
    controller.getSnapshot,
  )
  const locale = options.locale ?? 'uk'
  const labels = copy(locale)
  const diagnosticsRoot = useRef<HTMLDivElement>(null)
  const diagnosticsMounted = useRef(false)
  const [diagnosticsError, setDiagnosticsError] = useState(false)

  useEffect(() => {
    void controller.initialize()
    return () => controller.dispose()
  }, [controller])

  const openDiagnostics = (): void => {
    if (diagnosticsMounted.current || diagnosticsRoot.current === null) return
    diagnosticsMounted.current = true
    const inspectorOptions =
      options.databaseName === undefined
        ? { locale }
        : { locale, databaseName: options.databaseName }
    void mountFoundationInspector(diagnosticsRoot.current, inspectorOptions).catch(() =>
      setDiagnosticsError(true),
    )
  }

  const busy = snapshot.phase === 'pending' || snapshot.phase === 'loading'
  const accepted = snapshot.accepted
  return (
    <div className="editor-shell" aria-busy={busy}>
      <header className="editor-app-bar">
        <p className="eyebrow">
          {labels.product} <span className="eyebrow-separator">·</span> {labels.localOnly}
        </p>
        <h1 className="page-title">{labels.title}</h1>
        <p className="secondary-text">{labels.description}</p>
      </header>
      <main className="editor-main">
        <section className="editor-command-panel" aria-labelledby="editor-region-title">
          <h2 id="editor-region-title" className="visually-hidden">
            {labels.region}
          </h2>
          {accepted === null ? (
            <div className="empty-state" data-empty-document="">
              <h2 className="section-heading">{labels.emptyTitle}</h2>
              <p className="secondary-text">{labels.emptyBody}</p>
              <div className="action-group">
                <button
                  className="primary-action"
                  data-action="editor-create"
                  disabled={busy}
                  onClick={() => void controller.createSample()}
                >
                  {labels.create}
                </button>
                <button
                  data-action="editor-open-older"
                  disabled={busy}
                  onClick={() => void controller.openOlderSchema()}
                >
                  {labels.openOlder}
                </button>
                <button
                  data-action="editor-open-last"
                  disabled={busy}
                  onClick={() => void controller.reloadFromStorage()}
                >
                  {labels.openLast}
                </button>
              </div>
            </div>
          ) : (
            <>
              <div className="editor-toolbar" aria-label={labels.region}>
                <button
                  data-action="editor-replace"
                  disabled={busy}
                  onClick={() => void controller.replaceSelection()}
                >
                  {labels.replace}
                </button>
                <button
                  data-action="editor-undo"
                  disabled={busy || accepted.session.history.cursor === 0}
                  onClick={() => void controller.undo()}
                >
                  {labels.undo}
                </button>
                <button
                  data-action="editor-redo"
                  disabled={
                    busy || accepted.session.history.cursor >= accepted.session.history.entries.length
                  }
                  onClick={() => void controller.redo()}
                >
                  {labels.redo}
                </button>
                <button
                  data-action="editor-reload"
                  disabled={busy}
                  onClick={() => void controller.reloadFromStorage()}
                >
                  {labels.reload}
                </button>
                <button
                  data-action="editor-recover"
                  disabled={busy}
                  onClick={() => void controller.recoverFromStorage()}
                >
                  {labels.recover}
                </button>
              </div>
              <p className="durable-badge">{labels.durable}</p>
              <dl className="editor-metadata">
                <div>
                  <dt>{labels.revision}</dt>
                  <dd data-editor-revision="">{accepted.session.revision}</dd>
                </div>
                <div>
                  <dt>{labels.hash}</dt>
                  <dd className="hash-value" data-editor-hash="">
                    {accepted.session.canonicalHash}
                  </dd>
                </div>
              </dl>
              <section className="editor-document-region" aria-label={labels.region}>
                <SemanticDocument view={accepted.editor.view} />
              </section>
            </>
          )}
          <p className="status-region" data-editor-status="" aria-live="polite" aria-atomic="true">
            {snapshot.status}
          </p>
          {snapshot.errorCode === null ? null : (
            <p className="alert-region" role="alert" data-editor-error="">
              {labels.error}: {snapshot.errorCode}
            </p>
          )}
        </section>
        <details className="diagnostics-panel" onToggle={openDiagnostics}>
          <summary>{labels.diagnostics}</summary>
          <div ref={diagnosticsRoot} />
          {diagnosticsError ? (
            <p className="alert-region" role="alert">
              {labels.diagnosticsError}
            </p>
          ) : null}
        </details>
      </main>
    </div>
  )
}

export async function mountEditorApp(
  root: HTMLElement,
  options: EditorAppOptions = {},
): Promise<EditorController> {
  const controller = new EditorController(options)
  const reactRoot = createRoot(root)
  reactRoot.render(<EditorApp controller={controller} options={options} />)
  await controller.initialize()
  return controller
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

class EditorError extends Error {
  constructor(readonly code: string) {
    super(code)
    this.name = 'EditorError'
  }
}
