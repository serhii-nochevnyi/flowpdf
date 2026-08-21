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
} from '../persistence/indexeddb-store'
import { foundationInspectorEn } from './i18n/en'
import {
  foundationInspectorUk,
  type FoundationInspectorMessageKey,
} from './i18n/uk'

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
  readonly nextCommandTarget: LogicalPositionDto
  readonly history: HistoryStateDto
}

interface RevisionProvenanceDto {
  readonly documentId: string
  readonly revision: number
  readonly schemaVersion: number
  readonly canonicalHash: string
  readonly engine: {
    readonly engine: string
    readonly version: string
  }
  readonly lineage:
    | { readonly kind: 'created'; readonly createdAt: string }
    | {
        readonly kind: 'migrated'
        readonly sourceSchemaVersion: number
        readonly currentSchemaVersion: number
        readonly sourceCreatedAt: string
        readonly hops: readonly unknown[]
      }
  readonly sourceHashes: readonly string[]
  readonly previewExportProvenance: 'unavailableInPhaseOne'
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
  readonly commit: PersistenceCommitDto
}

interface RecoverResultDto {
  readonly session: SessionDto
  readonly view: InspectorViewDto
}

interface MigrateDocumentResultDto {
  readonly canonicalJson: string
  readonly canonicalHash: string
  readonly revisionProvenance: RevisionProvenanceDto
  readonly report: {
    readonly sourceSchemaVersion: number
    readonly currentSchemaVersion: number
  }
  readonly commit: MigrationPersistenceCommitDto | null
}

interface WasmBoundary {
  readonly default: () => Promise<unknown>
  readonly create_sample: (request: unknown) => ApiResponse<OperationResultDto>
  readonly apply_command: (request: unknown) => ApiResponse<OperationResultDto>
  readonly undo: (request: unknown) => ApiResponse<OperationResultDto>
  readonly redo: (request: unknown) => ApiResponse<OperationResultDto>
  readonly open_document: (request: unknown) => ApiResponse<MigrateDocumentResultDto>
  readonly commit_record: (request: unknown) => ApiResponse<PlannedPersistenceCommitDto>
  readonly query_document: (request: RecoveryRecordsDto) => ApiResponse<RecoverResultDto>
}

type ErrorPresentation = 'command' | 'stale' | 'recovery' | 'copy'

export interface FoundationInspectorSnapshot {
  readonly documentId: string
  readonly schemaVersion: number
  readonly revision: number
  readonly hash: string
  readonly locale: string
  readonly contentNodeCount: number
  readonly fieldCount: number
  readonly assetCount: number
  readonly revisionProvenance: RevisionProvenanceDto
  readonly audit: readonly AuditRecordDto[]
}

export interface FoundationInspectorController {
  whenIdle(): Promise<void>
  snapshot(): FoundationInspectorSnapshot
  reloadFromStorage(): Promise<FoundationInspectorSnapshot>
}

export interface FoundationInspectorOptions {
  readonly databaseName?: string
  readonly locale?: FoundationInspectorLocale
}

const GENERATED_WASM_MODULE = '../generated/flow_wasm.js'
const EMPTY_ASSET_HASH =
  'blake3:af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262'
let wasmPromise: Promise<WasmBoundary> | undefined

async function loadWasm(): Promise<WasmBoundary> {
  wasmPromise ??= import(/* @vite-ignore */ GENERATED_WASM_MODULE).then(async (module: unknown) => {
    const boundary = module as WasmBoundary
    await boundary.default()
    return boundary
  })
  return wasmPromise
}

export type FoundationInspectorLocale = 'uk' | 'en'

export const foundationInspectorMessages: Readonly<
  Record<
    FoundationInspectorLocale,
    Readonly<Record<FoundationInspectorMessageKey, string>>
  >
> = {
  uk: foundationInspectorUk,
  en: foundationInspectorEn,
}

function message(
  locale: FoundationInspectorLocale,
  key: FoundationInspectorMessageKey,
  parameters: Readonly<Record<string, string | number>> = {},
): string {
  let text = foundationInspectorMessages[locale][key]
  for (const [name, value] of Object.entries(parameters)) {
    text = text.replaceAll(`{${name}}`, String(value))
  }
  return text
}

class FoundationInspector implements FoundationInspectorController {
  private readonly elements: InspectorElements
  private session: SessionDto | undefined
  private view: InspectorViewDto | undefined
  private lastCommit: PersistenceCommitDto | undefined
  private hasDurableRecords = false
  private pending: Promise<void> = Promise.resolve()
  private busy = false
  private activeControl: HTMLButtonElement | undefined

  constructor(
    root: HTMLElement,
    private readonly wasm: WasmBoundary,
    private readonly store: IndexedDbDocumentStore,
    private readonly locale: FoundationInspectorLocale,
  ) {
    this.elements = createInspectorDom(root, locale)
    this.elements.create.addEventListener('click', () => {
      this.start(() => this.createSample(), this.elements.create)
    })
    this.elements.apply.addEventListener('click', () => {
      this.start(() => this.applyMutation(), this.elements.apply)
    })
    this.elements.openLast.addEventListener('click', () => {
      this.start(() => this.openLast(), this.elements.openLast, 'recovery')
    })
    this.elements.openOlder.addEventListener('click', () => {
      this.start(() => this.openOlderSchema(), this.elements.openOlder)
    })
    this.elements.stale.addEventListener('click', () => {
      this.start(() => this.applyStaleCommand(), this.elements.stale, 'stale')
    })
    this.elements.undo.addEventListener('click', () => {
      this.start(() => this.applyHistory('undo'), this.elements.undo)
    })
    this.elements.redo.addEventListener('click', () => {
      this.start(() => this.applyHistory('redo'), this.elements.redo)
    })
    this.elements.save.addEventListener('click', () => {
      this.start(() => this.save(), this.elements.save)
    })
    this.elements.reload.addEventListener('click', () => {
      this.start(() => this.restoreFromStorage('reload'), this.elements.reload, 'recovery')
    })
    this.elements.recover.addEventListener('click', () => {
      this.start(() => this.restoreFromStorage('recover'), this.elements.recover, 'recovery')
    })
    this.elements.copyDocumentId.addEventListener('click', () => {
      void this.copyValue(
        this.view?.documentId,
        'foundationInspector.copy.documentId.visible',
      )
    })
    this.elements.copyHash.addEventListener('click', () => {
      void this.copyValue(this.view?.canonicalHash, 'foundationInspector.copy.hash.visible')
    })
    root.addEventListener('keydown', (event) => this.handleKeyboard(event))
    this.render()
  }

  async initialize(): Promise<void> {
    const records = await this.store.loadRecords({ allowEmpty: true })
    this.hasDurableRecords = records.snapshots.length > 0
    this.render()
  }

  whenIdle(): Promise<void> {
    return this.pending
  }

  snapshot(): FoundationInspectorSnapshot {
    if (this.view === undefined) {
      throw new FoundationError('FLOW_NO_ACTIVE_DOCUMENT')
    }
    return {
      documentId: this.view.documentId,
      schemaVersion: this.view.schemaVersion,
      revision: this.view.revision,
      hash: this.view.canonicalHash,
      locale: this.view.locale,
      contentNodeCount: this.view.contentNodeCount,
      fieldCount: this.view.fieldCount,
      assetCount: this.view.assetCount,
      revisionProvenance: this.view.revisionProvenance,
      audit: this.view.audit,
    }
  }

  async reloadFromStorage(): Promise<FoundationInspectorSnapshot> {
    await this.restoreFromStorage('reload')
    return this.snapshot()
  }

  private start(
    operation: () => Promise<void>,
    control: HTMLButtonElement,
    errorKind: ErrorPresentation = 'command',
  ): void {
    if (this.busy || control.disabled) return
    this.activeControl = control
    this.pending = operation()
      .then(() => control.focus())
      .catch((error: unknown) => {
        this.setError(errorCode(error), errorKind)
        control.focus()
      })
  }

  private async createSample(): Promise<void> {
    this.setPending()
    try {
      const result = unwrap(
        this.wasm.create_sample({
          requestedLocale: 'uk-UA',
        }),
      )
      await this.persistPlanned(result.commit, 'creation')
      this.lastCommit = result.commit
      await this.publishRecoveredState(
        message(this.locale, 'foundationInspector.create.success'),
        result.session,
      )
    } finally {
      this.setBusy(false)
    }
  }

  private async applyMutation(): Promise<void> {
    const session = this.requireSession()
    this.setPending()
    try {
      const result = unwrap(
        this.wasm.apply_command({
          canonicalJson: session.canonicalJson,
          history: session.history,
          command: {
            commandId: newCommandId(),
            baseRevision: session.revision,
            modality: 'ui',
            issuedAt: '2026-08-14T00:00:01Z',
            kind: {
              type: 'insertText',
              target: session.nextCommandTarget,
              text: ' — typed mutation',
            },
          },
        }),
      )
      await this.persistPlanned(result.commit, 'committedTransaction')
      this.lastCommit = result.commit
      await this.publishRecoveredState(
        message(this.locale, 'foundationInspector.command.success', {
          revision: result.session.revision,
        }),
        result.session,
      )
    } finally {
      this.setBusy(false)
    }
  }

  private async applyStaleCommand(): Promise<void> {
    const session = this.requireSession()
    this.setPending()
    try {
      unwrap(
        this.wasm.apply_command({
          canonicalJson: session.canonicalJson,
          history: session.history,
          command: {
            commandId: newCommandId(),
            baseRevision: Math.max(0, session.revision - 1),
            modality: 'ui',
            issuedAt: '2026-08-14T00:00:01Z',
            kind: {
              type: 'insertText',
              target: session.nextCommandTarget,
              text: ' — stale diagnostic',
            },
          },
        }),
      )
      throw new FoundationError('FLOW_STALE_DIAGNOSTIC_ACCEPTED')
    } finally {
      this.setBusy(false)
    }
  }

  private async applyHistory(kind: 'undo' | 'redo'): Promise<void> {
    const session = this.requireSession()
    this.setPending()
    try {
      const request = {
        canonicalJson: session.canonicalJson,
        history: session.history,
        command: {
          commandId: newCommandId(),
          baseRevision: session.revision,
          modality: 'ui',
          issuedAt: '2026-08-14T00:00:01Z',
          kind: { type: kind },
        },
      }
      const result = unwrap(
        kind === 'undo' ? this.wasm.undo(request) : this.wasm.redo(request),
      )
      await this.persistPlanned(result.commit, 'committedTransaction')
      this.lastCommit = result.commit
      await this.publishRecoveredState(
        message(
          this.locale,
          kind === 'undo'
            ? 'foundationInspector.undo.success'
            : 'foundationInspector.redo.success',
          { revision: result.session.revision },
        ),
        result.session,
      )
    } finally {
      this.setBusy(false)
    }
  }

  private async openOlderSchema(): Promise<void> {
    this.setPending()
    try {
      const result = unwrap(
        this.wasm.open_document({
          fixture: 'supportedOlder',
          migrationId: newCommandId(),
          issuedAt: '2026-08-14T00:00:02Z',
          assets: [
            {
              recordFormatVersion: 1,
              contentHash: EMPTY_ASSET_HASH,
              bytes: [],
            },
          ],
        }),
      )
      if (result.commit === null) {
        throw new FoundationError('FLOW_MIGRATION_BOUNDARY_REQUIRED')
      }
      await this.store.commitMigration(result.commit)
      this.lastCommit = undefined
      await this.publishRecoveredState(
        message(this.locale, 'foundationInspector.migration.success', {
          sourceSchema: result.report.sourceSchemaVersion,
          currentSchema: result.report.currentSchemaVersion,
        }),
        {
          revision: result.revisionProvenance.revision,
          canonicalHash: result.canonicalHash,
        },
      )
    } finally {
      this.setBusy(false)
    }
  }

  private async openLast(): Promise<void> {
    await this.restoreFromStorage('open')
  }

  private async save(): Promise<void> {
    const session = this.requireSession()
    if (this.lastCommit === undefined) {
      throw new FoundationError('FLOW_NO_PENDING_COMMIT')
    }
    this.setPending()
    try {
      await this.persistPlanned(this.lastCommit, 'explicitLocalSave')
      await this.publishRecoveredState(
        message(this.locale, 'foundationInspector.save.success', {
          revision: session.revision,
        }),
        session,
      )
    } finally {
      this.setBusy(false)
    }
  }

  private async restoreFromStorage(mode: 'open' | 'reload' | 'recover'): Promise<void> {
    this.setPending()
    try {
      const records = await this.store.loadRecords()
      const recovered = unwrap(this.wasm.query_document(records))
      this.session = recovered.session
      this.view = recovered.view
      this.lastCommit = undefined
      const statusKey =
        mode === 'open'
          ? 'foundationInspector.open.success'
          : mode === 'reload'
            ? 'foundationInspector.reload.success'
            : 'foundationInspector.recover.success'
      this.setStatus(message(this.locale, statusKey, { revision: recovered.view.revision }))
      this.render()
    } finally {
      this.setBusy(false)
    }
  }

  private async publishRecoveredState(
    status: string,
    expected: Pick<SessionDto, 'revision' | 'canonicalHash'>,
  ): Promise<void> {
    const records = await this.store.loadRecords()
    const recovered = unwrap(this.wasm.query_document(records))
    if (
      recovered.session.revision !== expected.revision ||
      recovered.session.canonicalHash !== expected.canonicalHash
    ) {
      throw new FoundationError('FLOW_RESULT_REVISION_MISMATCH')
    }
    this.session = recovered.session
    this.view = recovered.view
    this.setStatus(status)
    this.render()
  }

  private async persistPlanned(
    commit: PersistenceCommitDto,
    reason: 'creation' | 'committedTransaction' | 'explicitLocalSave',
  ): Promise<void> {
    const records = await this.store.loadRecords({ allowEmpty: true })
    const planned = unwrap(
      this.wasm.commit_record({
        records,
        commit,
        reason,
      }),
    )
    await this.store.commit(planned)
    this.hasDurableRecords = true
  }

  private requireSession(): SessionDto {
    if (this.session === undefined) {
      throw new FoundationError('FLOW_NO_ACTIVE_DOCUMENT')
    }
    return this.session
  }

  private handleKeyboard(event: KeyboardEvent): void {
    if (
      this.busy ||
      isTextEditingTarget(event.target) ||
      !(event.ctrlKey || event.metaKey) ||
      event.altKey ||
      event.key.toLowerCase() !== 'z'
    ) {
      return
    }
    const kind = event.shiftKey ? 'redo' : 'undo'
    const control = kind === 'undo' ? this.elements.undo : this.elements.redo
    if (control.disabled) return
    event.preventDefault()
    control.focus()
    this.start(() => this.applyHistory(kind), control)
  }

  private async copyValue(
    value: string | undefined,
    labelKey:
      | 'foundationInspector.copy.documentId.visible'
      | 'foundationInspector.copy.hash.visible',
  ): Promise<void> {
    if (value === undefined) return
    try {
      await navigator.clipboard.writeText(value)
      this.setStatus(
        message(this.locale, 'foundationInspector.copy.success', {
          label: message(this.locale, labelKey),
        }),
      )
    } catch {
      this.setError('FLOW_CLIPBOARD_WRITE_FAILED', 'copy')
    }
  }

  private setPending(): void {
    this.setBusy(true)
    this.elements.alert.hidden = true
    this.elements.status.dataset.state = 'pending'
    this.elements.statusText.textContent = message(this.locale, 'foundationInspector.pending')
  }

  private setStatus(text: string): void {
    this.elements.alert.hidden = true
    this.elements.status.dataset.state = 'complete'
    this.elements.statusText.textContent = text
  }

  private setError(code: string, kind: ErrorPresentation = 'command'): void {
    this.elements.alert.hidden = false
    const key =
      kind === 'stale'
        ? 'foundationInspector.error.stale'
        : kind === 'recovery'
          ? 'foundationInspector.error.recovery'
          : kind === 'copy'
            ? 'foundationInspector.error.copy'
            : 'foundationInspector.error.command'
    this.elements.alert.textContent = message(this.locale, key, { code })
    this.elements.status.dataset.state = 'idle'
    this.elements.statusText.textContent = ''
  }

  private setBusy(busy: boolean): void {
    this.busy = busy
    this.updateControlAvailability()
    this.elements.root.setAttribute('aria-busy', String(busy))
  }

  private updateControlAvailability(): void {
    const hasSession = this.session !== undefined
    const cursor = this.session?.history.cursor ?? 0
    const entryCount = this.session?.history.entries.length ?? 0
    this.elements.create.disabled = false
    this.elements.openLast.disabled = !this.hasDurableRecords
    this.elements.openOlder.disabled = false
    this.elements.apply.disabled = !hasSession
    this.elements.stale.disabled = !hasSession
    this.elements.undo.disabled = !hasSession || cursor === 0
    this.elements.redo.disabled = !hasSession || cursor >= entryCount
    this.elements.save.disabled = !hasSession || this.lastCommit === undefined
    this.elements.reload.disabled = !hasSession || !this.hasDurableRecords
    this.elements.recover.disabled = !hasSession || !this.hasDurableRecords
    this.elements.copyDocumentId.disabled = !hasSession
    this.elements.copyHash.disabled = !hasSession
    if (this.busy && this.activeControl !== undefined) {
      this.activeControl.disabled = true
    }
  }

  private render(): void {
    const populated = this.view !== undefined
    this.elements.empty.hidden = populated
    this.elements.session.hidden = !populated
    this.elements.create.hidden = populated
    this.elements.openOlder.hidden = populated
    this.elements.openLast.hidden = populated || !this.hasDurableRecords
    for (const control of this.elements.sessionControls) control.hidden = !populated
    this.elements.auditEmpty.hidden = populated && this.view?.audit.length !== 0
    this.elements.provenanceUnavailable.hidden = false
    this.updateControlAvailability()

    if (this.view === undefined) {
      const unavailable = message(this.locale, 'foundationInspector.value.unavailable')
      this.elements.revision.textContent = unavailable
      this.elements.hash.textContent = unavailable
      this.elements.lastCommand.textContent = unavailable
      this.elements.auditBody.replaceChildren()
      this.elements.auditCount.textContent = auditCountText(this.locale, 0)
      return
    }
    this.elements.documentId.textContent = this.view.documentId
    this.elements.schemaVersion.textContent = String(this.view.schemaVersion)
    this.elements.revision.textContent = String(this.view.revision)
    this.elements.hash.textContent = this.view.canonicalHash
    this.elements.hash.title = this.view.canonicalHash
    this.elements.hash.setAttribute('aria-label', this.view.canonicalHash)
    this.elements.copyHash.setAttribute(
      'aria-label',
      message(this.locale, 'foundationInspector.copy.hash.label', {
        value: this.view.canonicalHash,
      }),
    )
    this.elements.documentId.title = this.view.documentId
    this.elements.documentId.setAttribute('aria-label', this.view.documentId)
    this.elements.copyDocumentId.setAttribute(
      'aria-label',
      message(this.locale, 'foundationInspector.copy.documentId.label', {
        value: this.view.documentId,
      }),
    )
    this.elements.locale.textContent = this.view.locale
    this.elements.durable.textContent = message(
      this.locale,
      'foundationInspector.durable.verified',
    )
    this.elements.summary.textContent = message(this.locale, 'foundationInspector.summary', {
      nodes: this.view.contentNodeCount,
      fields: this.view.fieldCount,
      assets: this.view.assetCount,
    })
    this.elements.provenance.textContent = provenanceText(
      this.locale,
      this.view.revisionProvenance,
    )
    this.elements.lastCommand.textContent =
      this.view.audit.at(-1)?.commandId ??
      message(this.locale, 'foundationInspector.value.unavailable')
    this.elements.auditEmpty.hidden = this.view.audit.length !== 0
    this.elements.auditCount.textContent = auditCountText(
      this.locale,
      this.view.audit.length,
    )
    this.elements.auditBody.replaceChildren(
      ...this.view.audit.map((entry, index) => auditRow(this.locale, entry, index)),
    )
  }
}

function provenanceText(
  locale: FoundationInspectorLocale,
  provenance: RevisionProvenanceDto,
): string {
  const lineage =
    provenance.lineage.kind === 'created'
      ? message(locale, 'foundationInspector.provenance.created', {
          createdAt: provenance.lineage.createdAt,
        })
      : message(locale, 'foundationInspector.provenance.migrated', {
          sourceSchema: provenance.lineage.sourceSchemaVersion,
          currentSchema: provenance.lineage.currentSchemaVersion,
        })
  const engine = message(locale, 'foundationInspector.provenance.engine', provenance.engine)
  return `${lineage} ${engine}`
}

function auditCountText(locale: FoundationInspectorLocale, count: number): string {
  const key =
    count === 0
      ? 'foundationInspector.audit.count.zero'
      : count === 1
        ? 'foundationInspector.audit.count.one'
        : 'foundationInspector.audit.count.many'
  return message(locale, key, { count })
}

function auditRow(
  locale: FoundationInspectorLocale,
  entry: AuditRecordDto,
  sourceIndex: number,
): HTMLTableRowElement {
  const row = document.createElement('tr')
  row.dataset.auditRow = ''
  row.dataset.auditId = entry.auditId
  row.dataset.transactionId = entry.transactionId
  row.dataset.revision = String(entry.newRevision)
  row.dataset.sourceIndex = String(sourceIndex)
  const identity = element(
    'span',
    'visually-hidden',
    message(locale, 'foundationInspector.audit.identity', {
      auditId: entry.auditId,
      transactionId: entry.transactionId,
    }),
  )

  const timestamp = element('time', 'audit-time', entry.timestamp)
  timestamp.dateTime = entry.timestamp
  row.append(
    tableCell(timestamp, identity),
    tableCell(document.createTextNode(`${entry.baseRevision} → ${entry.newRevision}`)),
    tableCell(
      document.createTextNode(auditAction(locale, entry)),
      auditMetadata(locale, entry),
    ),
    tableCell(document.createTextNode(auditModality(locale, entry.modality))),
    tableCell(document.createTextNode(auditOutcome(locale, entry))),
    tableCell(document.createTextNode(entry.commandId)),
  )
  return row
}

function auditAction(locale: FoundationInspectorLocale, entry: AuditRecordDto): string {
  if (entry.action.type === 'command') {
    return message(locale, `foundationInspector.audit.action.${entry.action.commandKind}`)
  }
  return message(locale, `foundationInspector.audit.action.${entry.action.type}`)
}

function auditModality(
  locale: FoundationInspectorLocale,
  modality: AuditRecordDto['modality'],
): string {
  return message(locale, `foundationInspector.audit.modality.${modality}`)
}

function auditOutcome(locale: FoundationInspectorLocale, entry: AuditRecordDto): string {
  return entry.outcome.kind === 'success'
    ? message(locale, 'foundationInspector.audit.outcome.success')
    : message(locale, 'foundationInspector.audit.outcome.failure', {
        code: entry.outcome.code,
      })
}

function auditMetadata(
  locale: FoundationInspectorLocale,
  entry: AuditRecordDto,
): HTMLElement {
  const text = entry.metadata
    .map((metadata) => {
      switch (metadata.kind) {
        case 'schemaVersion':
          return message(locale, 'foundationInspector.audit.metadata.schemaVersion', {
            version: metadata.value,
          })
        case 'migration':
          return message(locale, 'foundationInspector.audit.metadata.migration', {
            from: metadata.fromSchemaVersion,
            to: metadata.toSchemaVersion,
          })
        case 'recoveryVerified':
          return message(locale, 'foundationInspector.audit.metadata.recoveryVerified')
      }
    })
    .join('; ')
  return element(
    'span',
    'audit-metadata',
    text || message(locale, 'foundationInspector.value.unavailable'),
  )
}

function tableCell(...children: readonly Node[]): HTMLTableCellElement {
  const cell = document.createElement('td')
  cell.append(...children)
  return cell
}

export async function mountFoundationInspector(
  root: HTMLElement,
  options: FoundationInspectorOptions = {},
): Promise<FoundationInspectorController> {
  const wasm = await loadWasm()
  const inspector = new FoundationInspector(
    root,
    wasm,
    new IndexedDbDocumentStore(options.databaseName),
    options.locale ?? 'uk',
  )
  await inspector.initialize()
  return inspector
}

function unwrap<T>(response: ApiResponse<T>): T {
  if (!response.ok || response.value === null) {
    throw new FoundationError(response.error?.code ?? 'FLOW_UNKNOWN_CORE_ERROR')
  }
  return response.value
}

function newCommandId(): string {
  return globalThis.crypto.randomUUID()
}

class FoundationError extends Error {
  constructor(readonly code: string) {
    super(code)
    this.name = 'FoundationError'
  }
}

function errorCode(error: unknown): string {
  if (error instanceof FoundationError || error instanceof StorageError) {
    return error.code
  }
  return 'FLOW_UNEXPECTED_ERROR'
}

interface InspectorElements {
  readonly root: HTMLElement
  readonly create: HTMLButtonElement
  readonly openLast: HTMLButtonElement
  readonly openOlder: HTMLButtonElement
  readonly apply: HTMLButtonElement
  readonly stale: HTMLButtonElement
  readonly undo: HTMLButtonElement
  readonly redo: HTMLButtonElement
  readonly save: HTMLButtonElement
  readonly reload: HTMLButtonElement
  readonly recover: HTMLButtonElement
  readonly copyDocumentId: HTMLButtonElement
  readonly copyHash: HTMLButtonElement
  readonly controls: readonly HTMLButtonElement[]
  readonly sessionControls: readonly HTMLButtonElement[]
  readonly empty: HTMLElement
  readonly session: HTMLElement
  readonly documentId: HTMLElement
  readonly schemaVersion: HTMLElement
  readonly revision: HTMLElement
  readonly hash: HTMLElement
  readonly locale: HTMLElement
  readonly durable: HTMLElement
  readonly summary: HTMLElement
  readonly provenance: HTMLElement
  readonly provenanceUnavailable: HTMLElement
  readonly lastCommand: HTMLElement
  readonly auditBody: HTMLTableSectionElement
  readonly auditCount: HTMLElement
  readonly auditEmpty: HTMLElement
  readonly status: HTMLElement
  readonly statusText: HTMLElement
  readonly alert: HTMLElement
}

function createInspectorDom(
  root: HTMLElement,
  localeCode: FoundationInspectorLocale,
): InspectorElements {
  root.replaceChildren()
  root.classList.add('foundation-shell')
  root.lang = localeCode

  const header = element('header', 'app-header')
  const eyebrow = element('p', 'eyebrow')
  eyebrow.append(
    document.createTextNode(message(localeCode, 'foundationInspector.app.product')),
    element('span', 'eyebrow-separator', '·'),
    document.createTextNode(message(localeCode, 'foundationInspector.app.localOnly')),
  )
  header.append(
    eyebrow,
    element('h1', 'page-title', message(localeCode, 'foundationInspector.app.title')),
  )

  const main = element('main', 'foundation-grid')
  const commands = element('section', 'card command-panel')
  commands.setAttribute('aria-labelledby', 'commands-heading')
  const commandsHeading = element(
    'h2',
    'section-heading',
    message(localeCode, 'foundationInspector.section.commands'),
  )
  commandsHeading.id = 'commands-heading'
  const empty = element('div', 'empty-state')
  empty.dataset.emptyDocument = ''
  empty.append(
    element(
      'h3',
      'subsection-heading',
      message(localeCode, 'foundationInspector.empty.heading'),
    ),
    element(
      'p',
      'secondary-text',
      message(localeCode, 'foundationInspector.empty.body'),
    ),
  )
  const create = button(
    message(localeCode, 'foundationInspector.createSample'),
    'primary-action',
  )
  create.dataset.action = 'create-sample'
  const openLast = button(
    message(localeCode, 'foundationInspector.openLastLocal'),
    'secondary-action',
  )
  openLast.dataset.action = 'open-last'
  openLast.hidden = true
  const openOlder = button(
    message(localeCode, 'foundationInspector.openOlderSchema'),
    'secondary-action',
  )
  openOlder.dataset.action = 'open-older-schema'
  const apply = button(
    message(localeCode, 'foundationInspector.applyTestMutation'),
    'secondary-action',
  )
  apply.dataset.action = 'apply-mutation'
  const stale = button(
    message(localeCode, 'foundationInspector.testStaleCommand'),
    'secondary-action',
  )
  stale.dataset.action = 'stale-command'
  const undo = button(message(localeCode, 'foundationInspector.undo'), 'secondary-action')
  undo.dataset.action = 'undo'
  const redo = button(message(localeCode, 'foundationInspector.redo'), 'secondary-action')
  redo.dataset.action = 'redo'
  const save = button(message(localeCode, 'foundationInspector.save'), 'secondary-action')
  save.dataset.action = 'save'
  const reload = button(
    message(localeCode, 'foundationInspector.reload'),
    'secondary-action',
  )
  reload.dataset.action = 'reload'
  const recover = button(
    message(localeCode, 'foundationInspector.recover'),
    'secondary-action',
  )
  recover.dataset.action = 'recover'
  const sessionControls = [apply, stale, undo, redo, save, reload, recover]
  for (const control of sessionControls) control.hidden = true
  const controls = [create, openLast, openOlder, ...sessionControls]
  const documentActions = actionGroup(
    message(localeCode, 'foundationInspector.group.document'),
    create,
    openLast,
    openOlder,
  )
  const commandActions = actionGroup(
    message(localeCode, 'foundationInspector.group.command'),
    apply,
    stale,
  )
  const historyActions = actionGroup(
    message(localeCode, 'foundationInspector.group.history'),
    labelledShortcut(
      undo,
      message(localeCode, 'foundationInspector.keyboard.undoHint'),
    ),
    labelledShortcut(
      redo,
      message(localeCode, 'foundationInspector.keyboard.redoHint'),
    ),
  )
  const durabilityActions = actionGroup(
    message(localeCode, 'foundationInspector.group.durability'),
    save,
    reload,
    recover,
  )
  const status = element('p', 'status-region')
  status.dataset.durabilityStatus = ''
  status.dataset.state = 'idle'
  status.setAttribute('aria-live', 'polite')
  status.setAttribute('aria-atomic', 'true')
  const statusMarker = element('span', 'status-marker', '●')
  statusMarker.setAttribute('aria-hidden', 'true')
  const statusText = element('span', 'status-text')
  status.append(statusMarker, statusText)
  const alert = element('p', 'alert-region')
  alert.setAttribute('role', 'alert')
  alert.setAttribute('aria-atomic', 'true')
  alert.hidden = true
  commands.append(
    commandsHeading,
    empty,
    documentActions,
    commandActions,
    historyActions,
    durabilityActions,
    status,
    alert,
  )

  const session = element('section', 'card session-card')
  session.setAttribute('aria-labelledby', 'session-heading')
  session.hidden = true
  const sessionHeading = element(
    'h2',
    'section-heading',
    message(localeCode, 'foundationInspector.section.currentDocument'),
  )
  sessionHeading.id = 'session-heading'
  const summary = element('p', 'document-summary')
  summary.dataset.documentSummary = ''
  const metadata = element('dl', 'metadata-grid')
  const documentDefinition = addCopyDefinition(
    metadata,
    message(localeCode, 'foundationInspector.metadata.documentId'),
    message(localeCode, 'foundationInspector.copy.documentId.visible'),
    'document-id',
    'fullDocumentId',
  )
  const documentId = documentDefinition.value
  const copyDocumentId = documentDefinition.copy
  const schemaVersion = addDefinition(
    metadata,
    message(localeCode, 'foundationInspector.metadata.schemaVersion'),
  )
  const locale = addDefinition(
    metadata,
    message(localeCode, 'foundationInspector.metadata.locale'),
  )
  const durable = addDefinition(
    metadata,
    message(localeCode, 'foundationInspector.metadata.durable'),
    'durable-badge',
  )
  durable.textContent = message(localeCode, 'foundationInspector.durable.verified')
  session.append(sessionHeading, summary, metadata)

  const inspector = element('aside', 'card inspector-card')
  inspector.setAttribute(
    'aria-label',
    message(localeCode, 'foundationInspector.aside.label'),
  )
  const revisionHeading = element(
    'h2',
    'section-heading current-inspector',
    message(localeCode, 'foundationInspector.section.revision'),
  )
  const revisionList = element('dl', 'metadata-grid')
  const revision = addDefinition(
    revisionList,
    message(localeCode, 'foundationInspector.metadata.currentRevision'),
  )
  const hashDefinition = addCopyDefinition(
    revisionList,
    message(localeCode, 'foundationInspector.metadata.canonicalHash'),
    message(localeCode, 'foundationInspector.copy.hash.visible'),
    'revision-hash',
    'fullRevisionHash',
    'hash-value',
  )
  const hash = hashDefinition.value
  const copyHash = hashDefinition.copy
  const lastCommand = addDefinition(
    revisionList,
    message(localeCode, 'foundationInspector.metadata.lastCommand'),
    'identifier-value',
  )
  const provenanceHeading = element(
    'h2',
    'section-heading',
    message(localeCode, 'foundationInspector.section.provenance'),
  )
  const provenance = element('p', 'secondary-text')
  provenance.dataset.provenance = ''
  const provenanceUnavailable = element(
    'p',
    'boundary-note',
    message(localeCode, 'foundationInspector.provenance.noExport'),
  )
  provenanceUnavailable.dataset.provenanceUnavailable = ''
  const auditSection = element('section', 'audit-section')
  auditSection.setAttribute('aria-labelledby', 'audit-heading')
  const auditHeading = element(
    'h2',
    'section-heading',
    message(localeCode, 'foundationInspector.section.audit'),
  )
  auditHeading.id = 'audit-heading'
  const auditCount = element('p', 'audit-count', auditCountText(localeCode, 0))
  auditCount.dataset.auditCount = ''
  auditCount.setAttribute('aria-live', 'polite')
  auditCount.setAttribute('aria-atomic', 'true')
  const auditEmpty = element(
    'p',
    'secondary-text',
    message(localeCode, 'foundationInspector.audit.empty'),
  )
  auditEmpty.dataset.auditEmpty = ''
  const auditWrapper = element('div', 'audit-table-wrapper')
  const audit = document.createElement('table')
  audit.className = 'audit-table'
  audit.dataset.audit = ''
  const caption = element(
    'caption',
    'visually-hidden',
    message(localeCode, 'foundationInspector.section.audit'),
  )
  const auditHead = document.createElement('thead')
  const auditHeadRow = document.createElement('tr')
  for (const key of [
    'foundationInspector.audit.header.timestamp',
    'foundationInspector.audit.header.revision',
    'foundationInspector.audit.header.action',
    'foundationInspector.audit.header.source',
    'foundationInspector.audit.header.outcome',
    'foundationInspector.audit.header.commandId',
  ] as const) {
    const heading = element('th', '', message(localeCode, key))
    heading.scope = 'col'
    auditHeadRow.append(heading)
  }
  auditHead.append(auditHeadRow)
  const auditBody = document.createElement('tbody')
  audit.append(caption, auditHead, auditBody)
  auditWrapper.append(audit)
  auditSection.append(auditHeading, auditCount, auditEmpty, auditWrapper)
  inspector.append(
    revisionHeading,
    revisionList,
    provenanceHeading,
    provenance,
    provenanceUnavailable,
    auditSection,
  )

  main.append(elementWithChildren('div', 'primary-column', commands, session), inspector)
  root.append(header, main)

  return {
    root,
    create,
    openLast,
    openOlder,
    apply,
    stale,
    undo,
    redo,
    save,
    reload,
    recover,
    copyDocumentId,
    copyHash,
    controls,
    sessionControls,
    empty,
    session,
    documentId,
    schemaVersion,
    revision,
    hash,
    locale,
    durable,
    summary,
    provenance,
    provenanceUnavailable,
    lastCommand,
    auditBody,
    auditCount,
    auditEmpty,
    status,
    statusText,
    alert,
  }
}

function actionGroup(label: string, ...children: readonly Node[]): HTMLElement {
  const group = element('div', 'command-group')
  const heading = element('h3', 'command-group-heading', label)
  const actions = element('div', 'action-group')
  actions.append(...children)
  group.append(heading, actions)
  return group
}

function labelledShortcut(control: HTMLButtonElement, shortcut: string): HTMLElement {
  const wrapper = element('span', 'shortcut-control')
  const hint = element('span', 'keyboard-hint', shortcut)
  wrapper.append(control, hint)
  return wrapper
}

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag)
  node.className = className
  if (text !== undefined) {
    node.textContent = text
  }
  return node
}

function elementWithChildren<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  ...children: readonly Node[]
): HTMLElementTagNameMap[K] {
  const node = element(tag, className)
  node.append(...children)
  return node
}

function button(label: string, className: string): HTMLButtonElement {
  const control = element('button', className, label)
  control.type = 'button'
  return control
}

function addDefinition(list: HTMLElement, label: string, className = ''): HTMLElement {
  list.append(element('dt', 'metadata-label', label))
  const value = element('dd', className)
  list.append(value)
  return value
}

function addCopyDefinition(
  list: HTMLElement,
  label: string,
  copyLabel: string,
  copyKind: 'document-id' | 'revision-hash',
  fullValueDataset: 'fullDocumentId' | 'fullRevisionHash',
  className = '',
): { readonly value: HTMLElement; readonly copy: HTMLButtonElement } {
  list.append(element('dt', 'metadata-label', label))
  const definition = element('dd', 'copy-definition')
  const value = element('span', `identifier-value ${className}`.trim())
  value.dataset[fullValueDataset] = ''
  const copy = button(copyLabel, 'copy-action')
  copy.dataset.copy = copyKind
  definition.append(value, copy)
  list.append(definition)
  return { value, copy }
}

function isTextEditingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false
  return (
    target.matches('input, textarea, select') ||
    target.isContentEditable ||
    target.closest('[contenteditable="true"]') !== null
  )
}
