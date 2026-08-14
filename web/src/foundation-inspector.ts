import {
  IndexedDbDocumentStore,
  StorageError,
  type AuditRecordDto,
  type HistoryStateDto,
  type LogicalPositionDto,
  type PersistenceCommitDto,
  type RecoveryRecordsDto,
} from '../persistence/indexeddb-store'

interface ErrorDto {
  readonly code: string
  readonly message: string
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

interface InspectorViewDto {
  readonly documentId: string
  readonly schemaVersion: number
  readonly revision: number
  readonly canonicalHash: string
  readonly locale: string
  readonly documentSummary: string
  readonly provenance: string
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

interface WasmBoundary {
  readonly default: () => Promise<unknown>
  readonly create_sample: (request: unknown) => ApiResponse<OperationResultDto>
  readonly apply_command: (request: unknown) => ApiResponse<OperationResultDto>
  readonly recover_document: (request: RecoveryRecordsDto) => ApiResponse<RecoverResultDto>
}

export interface FoundationInspectorSnapshot {
  readonly documentId: string
  readonly schemaVersion: number
  readonly revision: number
  readonly hash: string
  readonly locale: string
  readonly documentSummary: string
  readonly provenance: string
  readonly audit: readonly AuditRecordDto[]
}

export interface FoundationInspectorController {
  whenIdle(): Promise<void>
  snapshot(): FoundationInspectorSnapshot
  reloadFromStorage(): Promise<FoundationInspectorSnapshot>
}

const GENERATED_WASM_MODULE = '../generated/flow_wasm.js'
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
  'foundationInspector.applyTestMutation': 'Застосувати тестову зміну',
  'foundationInspector.empty.heading': 'Документ ще не відкрито',
  'foundationInspector.empty.body':
    'Створіть тестовий документ, щоб перевірити ревізії та відновлення.',
  'foundationInspector.pending': 'Виконується…',
  'foundationInspector.create.success': 'Тестовий документ створено і збережено локально.',
  'foundationInspector.save.success': 'Збережено локально — ревізія {revision}.',
  'foundationInspector.error.command':
    'Команду не виконано. Дані не змінено. Код: {code}.',
  'foundationInspector.provenance.localSample': 'Створено локально з тестового зразка.',
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
      documentSummary: this.view.documentSummary,
      provenance: this.view.provenance,
      audit: this.view.audit,
    }
  }

  async reloadFromStorage(): Promise<FoundationInspectorSnapshot> {
    this.setPending()
    try {
      const records = await this.store.loadLatest()
      const recovered = unwrap(this.wasm.recover_document(records))
      this.session = recovered.session
      this.view = recovered.view
      this.setStatus(message('foundationInspector.save.success', { revision: recovered.view.revision }))
      this.render()
      return this.snapshot()
    } catch (error: unknown) {
      this.setError(errorCode(error))
      throw error
    } finally {
      this.setBusy(false)
    }
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
      await this.store.commit(result.commit)
      await this.publishRecoveredState(message('foundationInspector.create.success'))
    } finally {
      this.setBusy(false)
    }
  }

  private async applyMutation(): Promise<void> {
    if (this.session === undefined) {
      throw new FoundationError('FLOW_NO_ACTIVE_DOCUMENT')
    }
    this.setPending()
    try {
      const result = unwrap(
        this.wasm.apply_command({
          canonicalJson: this.session.canonicalJson,
          history: this.session.history,
          command: {
            commandId: commandIdForRevision(this.session.revision + 1),
            baseRevision: this.session.revision,
            modality: 'ui',
            issuedAt: '2026-08-14T00:00:01Z',
            kind: {
              type: 'insertText',
              target: this.session.nextCommandTarget,
              text: ' — typed mutation',
            },
          },
        }),
      )
      await this.store.commit(result.commit)
      await this.publishRecoveredState(
        message('foundationInspector.save.success', { revision: result.session.revision }),
      )
    } finally {
      this.setBusy(false)
    }
  }

  private async publishRecoveredState(status: string): Promise<void> {
    const records = await this.store.loadLatest()
    const recovered = unwrap(this.wasm.recover_document(records))
    this.session = recovered.session
    this.view = recovered.view
    this.setStatus(status)
    this.render()
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
    this.elements.create.disabled = busy
    this.elements.apply.disabled = busy || this.session === undefined
    this.elements.root.setAttribute('aria-busy', String(busy))
  }

  private render(): void {
    const populated = this.view !== undefined
    this.elements.empty.hidden = populated
    this.elements.session.hidden = !populated
    this.elements.apply.hidden = !populated
    this.elements.apply.disabled = !populated

    if (this.view === undefined) {
      return
    }
    this.elements.documentId.textContent = this.view.documentId
    this.elements.schemaVersion.textContent = String(this.view.schemaVersion)
    this.elements.revision.textContent = String(this.view.revision)
    this.elements.hash.textContent = this.view.canonicalHash
    this.elements.hash.setAttribute('aria-label', `Повний хеш ревізії ${this.view.canonicalHash}`)
    this.elements.locale.textContent = this.view.locale
    this.elements.summary.textContent = this.view.documentSummary
    this.elements.provenance.textContent = `${message('foundationInspector.provenance.localSample')} ${message('foundationInspector.provenance.noExport')}`
    this.elements.audit.replaceChildren(
      ...this.view.audit.map((entry) => {
        const item = document.createElement('li')
        item.textContent = `${entry.newRevision}: ${entry.commandType} · ${entry.modality} · ${entry.outcome}`
        return item
      }),
    )
  }
}

export async function mountFoundationInspector(
  root: HTMLElement,
): Promise<FoundationInspectorController> {
  const wasm = await loadWasm()
  return new FoundationInspector(root, wasm, new IndexedDbDocumentStore())
}

function unwrap<T>(response: ApiResponse<T>): T {
  if (!response.ok || response.value === null) {
    throw new FoundationError(response.error?.code ?? 'FLOW_UNKNOWN_CORE_ERROR')
  }
  return response.value
}

function commandIdForRevision(revision: number): string {
  return `00000000-0000-4000-8000-${String(200 + revision).padStart(12, '0')}`
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
  readonly apply: HTMLButtonElement
  readonly empty: HTMLElement
  readonly session: HTMLElement
  readonly documentId: HTMLElement
  readonly schemaVersion: HTMLElement
  readonly revision: HTMLElement
  readonly hash: HTMLElement
  readonly locale: HTMLElement
  readonly summary: HTMLElement
  readonly provenance: HTMLElement
  readonly audit: HTMLOListElement
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
  empty.append(
    element('h3', 'section-heading', message('foundationInspector.empty.heading')),
    element('p', 'secondary-text', message('foundationInspector.empty.body')),
  )
  const actions = element('div', 'action-group')
  const create = button(message('foundationInspector.createSample'), 'primary-action')
  create.dataset.action = 'create-sample'
  const apply = button(message('foundationInspector.applyTestMutation'), 'secondary-action')
  apply.dataset.action = 'apply-mutation'
  apply.hidden = true
  actions.append(create, apply)
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
  const auditSection = element('section', 'audit-section')
  auditSection.setAttribute('aria-labelledby', 'audit-heading')
  const auditHeading = element('h2', 'section-heading', 'Аудит')
  auditHeading.id = 'audit-heading'
  const audit = document.createElement('ol')
  audit.className = 'audit-list'
  audit.dataset.audit = ''
  auditSection.append(auditHeading, audit)
  inspector.append(revisionHeading, revisionList, provenanceHeading, provenance, auditSection)

  main.append(elementWithChildren('div', 'primary-column', commands, session), inspector)
  root.append(header, main)

  return {
    root,
    create,
    apply,
    empty,
    session,
    documentId,
    schemaVersion,
    revision,
    hash,
    locale,
    summary,
    provenance,
    audit,
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
