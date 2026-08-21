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

const messages = {
  'foundationInspector.createSample': 'Створити тестовий документ',
  'foundationInspector.openLastLocal': 'Відкрити останній локальний документ',
  'foundationInspector.openOlderSchema': 'Відкрити документ старішої схеми',
  'foundationInspector.applyTestMutation': 'Застосувати тестову зміну',
  'foundationInspector.testStaleCommand': 'Перевірити застарілу команду',
  'foundationInspector.undo': 'Скасувати',
  'foundationInspector.redo': 'Повторити',
  'foundationInspector.save': 'Зберегти локально',
  'foundationInspector.reload': 'Перезавантажити зі сховища',
  'foundationInspector.recover': 'Відновити останню стійку ревізію',
  'foundationInspector.empty.heading': 'Документ ще не відкрито',
  'foundationInspector.empty.body':
    'Створіть тестовий документ або відкрийте останній локальний документ, щоб перевірити ревізії та відновлення.',
  'foundationInspector.audit.empty': 'Записів аудиту ще немає.',
  'foundationInspector.pending': 'Виконується…',
  'foundationInspector.create.success': 'Тестовий документ створено і збережено локально.',
  'foundationInspector.open.success': 'Відкрито локальну ревізію {revision}.',
  'foundationInspector.migration.success':
    'Документ перенесено зі схеми {sourceSchema} до схеми {currentSchema}.',
  'foundationInspector.save.success': 'Збережено локально — ревізія {revision}.',
  'foundationInspector.error.command':
    'Команду не виконано. Дані не змінено. Код: {code}.',
  'foundationInspector.error.recovery':
    'Не вдалося безпечно відновити документ. Локальні дані не змінено. Код: {code}.',
  'foundationInspector.summary': 'Структура: блоків — {nodes}, полів — {fields}, ресурсів — {assets}.',
  'foundationInspector.provenance.created': 'Створено {createdAt}.',
  'foundationInspector.provenance.migrated':
    'Перенесено зі схеми {sourceSchema} до схеми {currentSchema}.',
  'foundationInspector.provenance.engine': 'Оброблено {engine} {version}.',
  'foundationInspector.provenance.noExport':
    'Походження попереднього перегляду або експорту буде доступне в наступній фазі.',
} as const

type MessageKey = keyof typeof messages

function message(key: MessageKey, parameters: Readonly<Record<string, string | number>> = {}): string {
  let text: string = messages[key]
  for (const [name, value] of Object.entries(parameters)) {
    text = text.replace(`{${name}}`, String(value))
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

  constructor(
    root: HTMLElement,
    private readonly wasm: WasmBoundary,
    private readonly store: IndexedDbDocumentStore,
  ) {
    this.elements = createInspectorDom(root)
    this.elements.create.addEventListener('click', () => {
      this.start(() => this.createSample())
    })
    this.elements.apply.addEventListener('click', () => {
      this.start(() => this.applyMutation())
    })
    this.elements.openLast.addEventListener('click', () => {
      this.start(() => this.openLast())
    })
    this.elements.openOlder.addEventListener('click', () => {
      this.start(() => this.openOlderSchema())
    })
    this.elements.stale.addEventListener('click', () => {
      this.start(() => this.applyStaleCommand())
    })
    this.elements.undo.addEventListener('click', () => {
      this.start(() => this.applyHistory('undo'))
    })
    this.elements.redo.addEventListener('click', () => {
      this.start(() => this.applyHistory('redo'))
    })
    this.elements.save.addEventListener('click', () => {
      this.start(() => this.save())
    })
    this.elements.reload.addEventListener('click', () => {
      this.start(() => this.restoreFromStorage('reload'))
    })
    this.elements.recover.addEventListener('click', () => {
      this.start(() => this.restoreFromStorage('recover'))
    })
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

  private start(operation: () => Promise<void>): void {
    this.pending = operation().catch((error: unknown) => {
      this.setError(errorCode(error))
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
        message('foundationInspector.create.success'),
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
        message('foundationInspector.save.success', { revision: result.session.revision }),
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
        message('foundationInspector.save.success', { revision: result.session.revision }),
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
        message('foundationInspector.migration.success', {
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
        message('foundationInspector.save.success', { revision: session.revision }),
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
      this.setStatus(
        mode === 'open'
          ? message('foundationInspector.open.success', { revision: recovered.view.revision })
          : message('foundationInspector.save.success', { revision: recovered.view.revision }),
      )
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

  private setPending(): void {
    this.setBusy(true)
    this.elements.alert.hidden = true
    this.elements.status.textContent = message('foundationInspector.pending')
  }

  private setStatus(text: string): void {
    this.elements.alert.hidden = true
    this.elements.status.textContent = text
  }

  private setError(code: string): void {
    this.elements.alert.hidden = false
    this.elements.alert.textContent = message('foundationInspector.error.command', { code })
    this.elements.status.textContent = ''
  }

  private setBusy(busy: boolean): void {
    for (const control of this.elements.controls) control.disabled = busy
    if (!busy) {
      const hasSession = this.session !== undefined
      const cursor = this.session?.history.cursor ?? 0
      const entryCount = this.session?.history.entries.length ?? 0
      this.elements.apply.disabled = !hasSession
      this.elements.stale.disabled = !hasSession
      this.elements.undo.disabled = !hasSession || cursor === 0
      this.elements.redo.disabled = !hasSession || cursor >= entryCount
      this.elements.save.disabled = !hasSession || this.lastCommit === undefined
      this.elements.reload.disabled = !hasSession || !this.hasDurableRecords
      this.elements.recover.disabled = !hasSession || !this.hasDurableRecords
    }
    this.elements.root.setAttribute('aria-busy', String(busy))
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
    this.setBusy(false)

    if (this.view === undefined) {
      this.elements.audit.replaceChildren()
      return
    }
    this.elements.documentId.textContent = this.view.documentId
    this.elements.schemaVersion.textContent = String(this.view.schemaVersion)
    this.elements.revision.textContent = String(this.view.revision)
    this.elements.hash.textContent = this.view.canonicalHash
    this.elements.hash.setAttribute('aria-label', `Повний хеш ревізії ${this.view.canonicalHash}`)
    this.elements.locale.textContent = this.view.locale
    this.elements.summary.textContent = message('foundationInspector.summary', {
      nodes: this.view.contentNodeCount,
      fields: this.view.fieldCount,
      assets: this.view.assetCount,
    })
    this.elements.provenance.textContent = provenanceText(this.view.revisionProvenance)
    this.elements.auditEmpty.hidden = this.view.audit.length !== 0
    this.elements.audit.replaceChildren(
      ...this.view.audit.map((entry) => {
        const item = document.createElement('li')
        const action =
          entry.action.type === 'command' ? entry.action.commandKind : entry.action.type
        item.textContent = `${entry.newRevision}: ${action} · ${entry.modality} · ${entry.outcome.kind}`
        return item
      }),
    )
  }
}

function provenanceText(provenance: RevisionProvenanceDto): string {
  const lineage =
    provenance.lineage.kind === 'created'
      ? message('foundationInspector.provenance.created', {
          createdAt: provenance.lineage.createdAt,
        })
      : message('foundationInspector.provenance.migrated', {
          sourceSchema: provenance.lineage.sourceSchemaVersion,
          currentSchema: provenance.lineage.currentSchemaVersion,
        })
  const engine = message('foundationInspector.provenance.engine', provenance.engine)
  return `${lineage} ${engine}`
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
  readonly controls: readonly HTMLButtonElement[]
  readonly sessionControls: readonly HTMLButtonElement[]
  readonly empty: HTMLElement
  readonly session: HTMLElement
  readonly documentId: HTMLElement
  readonly schemaVersion: HTMLElement
  readonly revision: HTMLElement
  readonly hash: HTMLElement
  readonly locale: HTMLElement
  readonly summary: HTMLElement
  readonly provenance: HTMLElement
  readonly provenanceUnavailable: HTMLElement
  readonly audit: HTMLOListElement
  readonly auditEmpty: HTMLElement
  readonly status: HTMLElement
  readonly alert: HTMLElement
}

function createInspectorDom(root: HTMLElement): InspectorElements {
  root.replaceChildren()
  root.classList.add('foundation-shell')

  const header = element('header', 'app-header')
  header.append(
    element('p', 'eyebrow', 'FlowPDF · Локальна перевірка'),
    element('h1', 'page-title', 'Інспектор основи'),
  )

  const main = element('main', 'foundation-grid')
  const commands = element('section', 'card command-panel')
  commands.setAttribute('aria-labelledby', 'commands-heading')
  const commandsHeading = element('h2', 'section-heading', 'Команди перевірки')
  commandsHeading.id = 'commands-heading'
  const empty = element('div', 'empty-state')
  empty.dataset.emptyDocument = ''
  empty.append(
    element('h3', 'section-heading', message('foundationInspector.empty.heading')),
    element('p', 'secondary-text', message('foundationInspector.empty.body')),
  )
  const actions = element('div', 'action-group')
  const create = button(message('foundationInspector.createSample'), 'primary-action')
  create.dataset.action = 'create-sample'
  const openLast = button(message('foundationInspector.openLastLocal'), 'secondary-action')
  openLast.dataset.action = 'open-last'
  openLast.hidden = true
  const openOlder = button(message('foundationInspector.openOlderSchema'), 'secondary-action')
  openOlder.dataset.action = 'open-older-schema'
  const apply = button(message('foundationInspector.applyTestMutation'), 'secondary-action')
  apply.dataset.action = 'apply-mutation'
  const stale = button(message('foundationInspector.testStaleCommand'), 'secondary-action')
  stale.dataset.action = 'stale-command'
  const undo = button(message('foundationInspector.undo'), 'secondary-action')
  undo.dataset.action = 'undo'
  const redo = button(message('foundationInspector.redo'), 'secondary-action')
  redo.dataset.action = 'redo'
  const save = button(message('foundationInspector.save'), 'secondary-action')
  save.dataset.action = 'save'
  const reload = button(message('foundationInspector.reload'), 'secondary-action')
  reload.dataset.action = 'reload'
  const recover = button(message('foundationInspector.recover'), 'secondary-action')
  recover.dataset.action = 'recover'
  const sessionControls = [apply, stale, undo, redo, save, reload, recover]
  for (const control of sessionControls) control.hidden = true
  const controls = [create, openLast, openOlder, ...sessionControls]
  actions.append(...controls)
  const status = element('p', 'status-region')
  status.dataset.durabilityStatus = ''
  status.setAttribute('aria-live', 'polite')
  const alert = element('p', 'alert-region')
  alert.setAttribute('role', 'alert')
  alert.hidden = true
  commands.append(commandsHeading, empty, actions, status, alert)

  const session = element('section', 'card session-card')
  session.setAttribute('aria-labelledby', 'session-heading')
  session.hidden = true
  const sessionHeading = element('h2', 'section-heading', 'Поточний документ')
  sessionHeading.id = 'session-heading'
  const summary = element('p', 'document-summary')
  summary.dataset.documentSummary = ''
  const metadata = element('dl', 'metadata-grid')
  const documentId = addDefinition(metadata, 'ID документа')
  const schemaVersion = addDefinition(metadata, 'Версія схеми')
  const locale = addDefinition(metadata, 'Локаль')
  session.append(sessionHeading, summary, metadata)

  const inspector = element('aside', 'card inspector-card')
  inspector.setAttribute('aria-label', 'Інспектор документа')
  const revisionHeading = element('h2', 'section-heading current-inspector', 'Ревізія')
  const revisionList = element('dl', 'metadata-grid')
  const revision = addDefinition(revisionList, 'Поточна ревізія')
  const hash = addDefinition(revisionList, 'Канонічний хеш', 'hash-value')
  const provenanceHeading = element('h2', 'section-heading', 'Походження')
  const provenance = element('p', 'secondary-text')
  provenance.dataset.provenance = ''
  const provenanceUnavailable = element(
    'p',
    'secondary-text',
    message('foundationInspector.provenance.noExport'),
  )
  provenanceUnavailable.dataset.provenanceUnavailable = ''
  const auditSection = element('section', 'audit-section')
  auditSection.setAttribute('aria-labelledby', 'audit-heading')
  const auditHeading = element('h2', 'section-heading', 'Аудит')
  auditHeading.id = 'audit-heading'
  const audit = document.createElement('ol')
  audit.className = 'audit-list'
  audit.dataset.audit = ''
  const auditEmpty = element(
    'p',
    'secondary-text',
    message('foundationInspector.audit.empty'),
  )
  auditEmpty.dataset.auditEmpty = ''
  auditSection.append(auditHeading, auditEmpty, audit)
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
    controls,
    sessionControls,
    empty,
    session,
    documentId,
    schemaVersion,
    revision,
    hash,
    locale,
    summary,
    provenance,
    provenanceUnavailable,
    audit,
    auditEmpty,
    status,
    alert,
  }
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
