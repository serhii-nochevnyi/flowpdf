export interface LogicalPositionDto {
  readonly nodeId: string
  readonly utf16Offset: number
  readonly affinity: 'forward' | 'backward'
}

export type AuditCommandKindDto =
  | 'create'
  | 'insertText'
  | 'replaceText'
  | 'deleteText'
  | 'setNodeStyle'
  | 'insertNode'
  | 'deleteNode'
  | 'setField'
  | 'batch'
  | 'undo'
  | 'redo'
  | 'recovery'

export type AuditActionDto =
  | { readonly type: 'create' }
  | { readonly type: 'command'; readonly commandKind: AuditCommandKindDto }
  | { readonly type: 'migration' }
  | { readonly type: 'recovery' }

export type AuditOutcomeDto =
  | { readonly kind: 'success' }
  | { readonly kind: 'failure'; readonly code: string }

export type AuditMetadataDto =
  | { readonly kind: 'schemaVersion'; readonly value: number }
  | {
      readonly kind: 'migration'
      readonly fromSchemaVersion: number
      readonly toSchemaVersion: number
    }
  | { readonly kind: 'recoveryVerified' }

export interface AuditRecordDto {
  readonly recordFormatVersion: number
  readonly auditId: string
  readonly documentId: string
  readonly transactionId: string
  readonly commandId: string
  readonly baseRevision: number
  readonly newRevision: number
  readonly durableSequence: number
  readonly timestamp: string
  readonly action: AuditActionDto
  readonly modality: 'ui' | 'keyboard' | 'voice' | 'api' | 'system'
  readonly outcome: AuditOutcomeDto
  readonly metadata: readonly AuditMetadataDto[]
}

export interface SnapshotRecordDto {
  readonly recordFormatVersion: number
  readonly documentId: string
  readonly revision: number
  readonly schemaVersion: number
  readonly canonicalJson: string
  readonly canonicalHash: string
  readonly history: HistoryStateDto
  readonly migrationBoundary?: {
    readonly recordFormatVersion: number
    readonly migrationId: string
    readonly issuedAt: string
    readonly documentId: string
    readonly revision: number
    readonly sourceSchemaVersion: number
    readonly currentSchemaVersion: number
    readonly sourceCanonicalHash: string
    readonly migratedCanonicalHash: string
    readonly hops: readonly {
      readonly fromVersion: number
      readonly toVersion: number
    }[]
  }
}

export interface HistoryStateDto {
  readonly entries: readonly unknown[]
  readonly cursor: number
  readonly seenCommandIds: readonly string[]
}

export interface TransactionRecordDto {
  readonly recordFormatVersion: number
  readonly transactionId: string
  readonly documentId: string
  readonly schemaVersion: number
  readonly commandId: string
  readonly baseRevision: number
  readonly newRevision: number
  readonly commandType: string
  readonly modality: 'ui' | 'keyboard' | 'voice' | 'api' | 'system'
  readonly issuedAt: string
  readonly beforeHash: string
  readonly afterHash: string
  readonly forwardOperations: readonly unknown[]
  readonly inverseOperations: readonly unknown[]
  readonly anchorMapping: unknown
  readonly historyEffect: unknown
}

export interface PersistenceCommitDto {
  readonly replaceExisting: boolean
  readonly snapshot: SnapshotRecordDto
  readonly transaction: TransactionRecordDto
  readonly audit: AuditRecordDto
  readonly assets: readonly AssetRecordDto[]
}

export interface PlannedPersistenceCommitDto {
  readonly replaceExisting: boolean
  readonly snapshot: SnapshotRecordDto | null
  readonly transaction: TransactionRecordDto
  readonly audit: AuditRecordDto
  readonly assets: readonly AssetRecordDto[]
}

export interface AssetRecordDto {
  readonly recordFormatVersion: number
  readonly contentHash: string
  readonly bytes: readonly number[]
}

export interface MigrationSourceRecordDto {
  readonly recordFormatVersion: number
  readonly documentId: string
  readonly schemaVersion: number
  readonly canonicalJson: string
  readonly canonicalHash: string
}

export interface MigrationPersistenceCommitDto {
  readonly snapshot: SnapshotRecordDto
  readonly audit: AuditRecordDto
  readonly assets: readonly AssetRecordDto[]
  readonly source: MigrationSourceRecordDto
}

export interface RecoveryRecordsDto {
  readonly snapshots: readonly SnapshotRecordDto[]
  readonly transactions: readonly TransactionRecordDto[]
  readonly audits: readonly AuditRecordDto[]
  readonly assets: readonly AssetRecordDto[]
  readonly sources: readonly MigrationSourceRecordDto[]
}

const DEFAULT_DATABASE_NAME = 'flowpdf-foundation'
const DATABASE_VERSION = 5
const RECORD_STORE_VERSION = 3
const SNAPSHOTS = `snapshots-v${RECORD_STORE_VERSION}`
const TRANSACTIONS = `transactions-v${RECORD_STORE_VERSION}`
const AUDITS = `audits-v${RECORD_STORE_VERSION}`
const ASSETS = `assets-v${RECORD_STORE_VERSION}`
const SOURCES = `migration-sources-v${RECORD_STORE_VERSION}`
const METADATA = 'storage-metadata'
const HEADS = 'document-heads'
const LEGACY_MIGRATION_REQUIRED = 'legacy-migration-required'
const HEAD_BOOTSTRAP_REQUIRED = 'head-bootstrap-required'
const LEGACY_STORES = ['snapshots', 'transactions', 'audits', 'assets', 'migration-sources']

interface StorageMetadata {
  readonly key: string
  readonly value: boolean
}

export interface DocumentHeadDto {
  readonly documentId: string
  readonly revision: number
  readonly canonicalHash: string
}

interface HeadTransition {
  readonly expected: DocumentHeadDto | null
  readonly resulting: DocumentHeadDto
}

interface StoredEnvelope<T> {
  readonly physicalKey: string
  readonly record: T
}

type SameIdentity<T> = (left: T, right: T) => boolean

interface GuardedRecord {
  readonly envelope: StoredEnvelope<unknown>
  readonly sameIdentity: SameIdentity<unknown>
}

interface GuardedStoreWrites {
  readonly storeName: string
  readonly records: readonly GuardedRecord[]
}

export class IndexedDbDocumentStore {
  private databasePromise: Promise<IDBDatabase> | undefined

  constructor(private readonly databaseName = DEFAULT_DATABASE_NAME) {}

  async commit(commit: PlannedPersistenceCommitDto): Promise<void> {
    const snapshot =
      commit.snapshot === null
        ? null
        : await recordEnvelope(commit.snapshot, 'FLOW_STORAGE_WRITE_FAILED')
    const transactionRecord = await recordEnvelope(
      commit.transaction,
      'FLOW_STORAGE_WRITE_FAILED',
    )
    const audit = await recordEnvelope(commit.audit, 'FLOW_STORAGE_WRITE_FAILED')
    const assets = await Promise.all(
      commit.assets.map((asset) => recordEnvelope(asset, 'FLOW_STORAGE_WRITE_FAILED')),
    )
    const database = await this.open()

    await commitGuardedRecords(
      database,
      [SNAPSHOTS, TRANSACTIONS, AUDITS, ASSETS, SOURCES],
      [
        {
          storeName: SNAPSHOTS,
          records:
            snapshot === null
              ? []
              : [guardedRecord(snapshot, sameSnapshotIdentity)],
        },
        {
          storeName: TRANSACTIONS,
          records: [guardedRecord(transactionRecord, sameTransactionIdentity)],
        },
        {
          storeName: AUDITS,
          records: [guardedRecord(audit, sameAuditIdentity)],
        },
        {
          storeName: ASSETS,
          records: assets.map((asset) => guardedRecord(asset, sameAssetIdentity)),
        },
        { storeName: SOURCES, records: [] },
      ],
      {
        expected:
          commit.transaction.baseRevision === 0
            ? null
            : {
                documentId: commit.transaction.documentId,
                revision: commit.transaction.baseRevision,
                canonicalHash: commit.transaction.beforeHash,
              },
        resulting: {
          documentId: commit.transaction.documentId,
          revision: commit.transaction.newRevision,
          canonicalHash: commit.transaction.afterHash,
        },
      },
    )
  }

  async commitMigration(commit: MigrationPersistenceCommitDto): Promise<void> {
    const [snapshot, audit, source, assets] = await Promise.all([
      recordEnvelope(commit.snapshot, 'FLOW_STORAGE_WRITE_FAILED'),
      recordEnvelope(commit.audit, 'FLOW_STORAGE_WRITE_FAILED'),
      recordEnvelope(commit.source, 'FLOW_STORAGE_WRITE_FAILED'),
      Promise.all(
        commit.assets.map((asset) => recordEnvelope(asset, 'FLOW_STORAGE_WRITE_FAILED')),
      ),
    ])
    const database = await this.open()

    await commitGuardedRecords(
      database,
      [SNAPSHOTS, TRANSACTIONS, AUDITS, ASSETS, SOURCES],
      [
        {
          storeName: SNAPSHOTS,
          records: [guardedRecord(snapshot, sameSnapshotIdentity)],
        },
        { storeName: TRANSACTIONS, records: [] },
        {
          storeName: AUDITS,
          records: [guardedRecord(audit, sameAuditIdentity)],
        },
        {
          storeName: ASSETS,
          records: assets.map((asset) => guardedRecord(asset, sameAssetIdentity)),
        },
        {
          storeName: SOURCES,
          records: [guardedRecord(source, sameMigrationSourceIdentity)],
        },
      ],
      {
        expected: null,
        resulting: {
          documentId: commit.snapshot.documentId,
          revision: commit.snapshot.revision,
          canonicalHash: commit.snapshot.canonicalHash,
        },
      },
    )
  }

  async loadRecords(options: { readonly allowEmpty?: boolean } = {}): Promise<RecoveryRecordsDto> {
    const database = await this.open()
    const transaction = database.transaction(
      [SNAPSHOTS, TRANSACTIONS, AUDITS, ASSETS, SOURCES, METADATA],
      'readonly',
    )
    const snapshotsRequest = transaction.objectStore(SNAPSHOTS).getAll()
    const transactionsRequest = transaction.objectStore(TRANSACTIONS).getAll()
    const auditsRequest = transaction.objectStore(AUDITS).getAll()
    const assetsRequest = transaction.objectStore(ASSETS).getAll()
    const sourcesRequest = transaction.objectStore(SOURCES).getAll()
    const migrationRequiredRequest = transaction
      .objectStore(METADATA)
      .get(LEGACY_MIGRATION_REQUIRED)

    const [
      snapshotEnvelopes,
      transactionEnvelopes,
      auditEnvelopes,
      assetEnvelopes,
      sourceEnvelopes,
      migrationRequired,
    ] = await Promise.all([
      requestResult<StoredEnvelope<SnapshotRecordDto>[]>(snapshotsRequest),
      requestResult<StoredEnvelope<TransactionRecordDto>[]>(transactionsRequest),
      requestResult<StoredEnvelope<AuditRecordDto>[]>(auditsRequest),
      requestResult<StoredEnvelope<AssetRecordDto>[]>(assetsRequest),
      requestResult<StoredEnvelope<MigrationSourceRecordDto>[]>(sourcesRequest),
      requestResult<StorageMetadata | undefined>(migrationRequiredRequest),
      transactionComplete(transaction),
    ])
    const snapshots = snapshotEnvelopes.map(({ record }) => record)
    const transactions = transactionEnvelopes.map(({ record }) => record)
    const audits = auditEnvelopes.map(({ record }) => record)
    const assets = assetEnvelopes.map(({ record }) => record)
    const sources = sourceEnvelopes.map(({ record }) => record)

    if (migrationRequired?.value === true) {
      throw storageError('FLOW_STORAGE_MIGRATION_REQUIRED')
    }

    const isEmpty =
      snapshots.length === 0 &&
      transactions.length === 0 &&
      audits.length === 0 &&
      assets.length === 0 &&
      sources.length === 0
    if (isEmpty && options.allowEmpty !== true) {
      throw storageError('FLOW_STORAGE_EMPTY')
    }

    return {
      snapshots,
      transactions,
      audits,
      assets,
      sources,
    }
  }

  async installRecoveredHead(
    recoveredRecords: RecoveryRecordsDto,
    head: DocumentHeadDto,
  ): Promise<void> {
    const database = await this.open()
    await new Promise<void>((resolve, reject) => {
      const storeNames = [SNAPSHOTS, TRANSACTIONS, AUDITS, ASSETS, SOURCES] as const
      const transaction = database.transaction(
        [...storeNames, METADATA, HEADS],
        'readwrite',
        { durability: 'strict' },
      )
      const currentRecords = new Map<string, readonly unknown[]>()
      let currentHead: DocumentHeadDto | undefined
      let metadata: readonly StorageMetadata[] = []
      let readsRemaining = storeNames.length + 2
      let failure: StorageError | undefined

      transaction.oncomplete = () => resolve()
      transaction.onabort = () =>
        reject(failure ?? storageError('FLOW_STORAGE_ABORTED', transaction.error))
      transaction.onerror = () => {
        failure ??= storageError('FLOW_STORAGE_WRITE_FAILED', transaction.error)
      }

      const abort = (error: StorageError): void => {
        failure ??= error
        try {
          transaction.abort()
        } catch {
          // The transaction may already be aborting because a request failed.
        }
      }
      const ready = (): void => {
        readsRemaining -= 1
        if (readsRemaining !== 0) return
        try {
          if (metadata.some(({ key, value }) => key === LEGACY_MIGRATION_REQUIRED && value)) {
            throw storageError('FLOW_STORAGE_MIGRATION_REQUIRED')
          }
          if (currentHead !== undefined && !sameHead(currentHead, head)) {
            throw storageError('FLOW_STORE_HEAD_CONFLICT')
          }
          const current = recoveryRecordsFromEnvelopes(currentRecords)
          if (deterministicJson(current) !== deterministicJson(recoveredRecords)) {
            throw storageError('FLOW_STORAGE_HEAD_REQUIRED')
          }
          const headIsBound =
            current.snapshots.some(
              (snapshot) =>
                snapshot.documentId === head.documentId &&
                snapshot.revision === head.revision &&
                snapshot.canonicalHash === head.canonicalHash,
            ) ||
            current.transactions.some(
              (record) =>
                record.documentId === head.documentId &&
                record.newRevision === head.revision &&
                record.afterHash === head.canonicalHash,
            )
          if (!headIsBound) throw storageError('FLOW_STORAGE_HEAD_REQUIRED')
          transaction.objectStore(HEADS).put(head)
          transaction.objectStore(METADATA).delete(HEAD_BOOTSTRAP_REQUIRED)
        } catch (error: unknown) {
          abort(
            error instanceof StorageError
              ? error
              : storageError('FLOW_STORAGE_WRITE_FAILED', error),
          )
        }
      }

      for (const storeName of storeNames) {
        const request = transaction.objectStore(storeName).getAll()
        request.onsuccess = () => {
          currentRecords.set(storeName, request.result as readonly StoredEnvelope<unknown>[])
          ready()
        }
        request.onerror = () =>
          abort(storageError('FLOW_STORAGE_READ_FAILED', request.error))
      }
      const headRequest = transaction.objectStore(HEADS).get(head.documentId)
      headRequest.onsuccess = () => {
        currentHead = headRequest.result as DocumentHeadDto | undefined
        ready()
      }
      headRequest.onerror = () =>
        abort(storageError('FLOW_STORAGE_READ_FAILED', headRequest.error))
      const metadataRequest = transaction.objectStore(METADATA).getAll()
      metadataRequest.onsuccess = () => {
        metadata = metadataRequest.result as StorageMetadata[]
        ready()
      }
      metadataRequest.onerror = () =>
        abort(storageError('FLOW_STORAGE_READ_FAILED', metadataRequest.error))
    })
  }

  private open(): Promise<IDBDatabase> {
    if (this.databasePromise !== undefined) return this.databasePromise

    const pending = new Promise<IDBDatabase>((resolve, reject) => {
      let request: IDBOpenDBRequest
      try {
        request = indexedDB.open(this.databaseName, DATABASE_VERSION)
      } catch (error: unknown) {
        reject(storageError('FLOW_STORAGE_OPEN_FAILED', error))
        return
      }
      request.onupgradeneeded = (event) => {
        const database = request.result
        for (const storeName of [SNAPSHOTS, TRANSACTIONS, AUDITS, ASSETS, SOURCES]) {
          if (!database.objectStoreNames.contains(storeName)) {
            database.createObjectStore(storeName, { keyPath: 'physicalKey' })
          }
        }
        if (!database.objectStoreNames.contains(METADATA)) {
          database.createObjectStore(METADATA, { keyPath: 'key' })
        }
        if (!database.objectStoreNames.contains(HEADS)) {
          database.createObjectStore(HEADS, { keyPath: 'documentId' })
        }
        const upgrade = request.transaction
        if (upgrade === null) return
        const metadata = upgrade.objectStore(METADATA)
        for (const storeName of LEGACY_STORES) {
          if (!database.objectStoreNames.contains(storeName)) continue
          const count = upgrade.objectStore(storeName).count()
          count.onsuccess = () => {
            if (count.result > 0) {
              metadata.put({ key: LEGACY_MIGRATION_REQUIRED, value: true })
            }
          }
        }
        if (event.oldVersion > 0) {
          for (const storeName of [SNAPSHOTS, TRANSACTIONS, AUDITS, ASSETS, SOURCES]) {
            const count = upgrade.objectStore(storeName).count()
            count.onsuccess = () => {
              if (count.result > 0) {
                metadata.put({ key: HEAD_BOOTSTRAP_REQUIRED, value: true })
              }
            }
          }
        }
      }
      request.onsuccess = () => {
        const database = request.result
        database.onversionchange = () => {
          database.close()
          if (this.databasePromise === pending) this.databasePromise = undefined
        }
        resolve(database)
      }
      request.onerror = () => reject(storageError('FLOW_STORAGE_OPEN_FAILED', request.error))
      // A blocked upgrade is not terminal: IndexedDB can still complete this
      // request once older connections close.
    })
    this.databasePromise = pending
    void pending.catch(() => {
      if (this.databasePromise === pending) this.databasePromise = undefined
    })
    return pending
  }
}

async function recordEnvelope<T>(record: T, errorCode: string): Promise<StoredEnvelope<T>> {
  try {
    const bytes = new TextEncoder().encode(deterministicJson(record))
    const digest = await globalThis.crypto.subtle.digest('SHA-256', bytes)
    const physicalKey = Array.from(new Uint8Array(digest), (byte) =>
      byte.toString(16).padStart(2, '0'),
    ).join('')
    return { physicalKey, record }
  } catch (error: unknown) {
    throw storageError(errorCode, error)
  }
}

function guardedRecord<T>(
  envelope: StoredEnvelope<T>,
  sameIdentity: SameIdentity<T>,
): GuardedRecord {
  return {
    envelope: envelope as StoredEnvelope<unknown>,
    sameIdentity: (left, right) => sameIdentity(left as T, right as T),
  }
}

function commitGuardedRecords(
  database: IDBDatabase,
  storeNames: readonly string[],
  groups: readonly GuardedStoreWrites[],
  headTransition: HeadTransition,
): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    const transaction = database.transaction(
      [...new Set([...storeNames, METADATA, HEADS])],
      'readwrite',
      { durability: 'strict' },
    )
    const existingByStore = new Map<string, readonly StoredEnvelope<unknown>[]>()
    const populatedGroups = groups.filter(({ records }) => records.length > 0)
    let readsRemaining = groups.length + 2
    let failure: StorageError | undefined
    let currentHead: DocumentHeadDto | undefined

    transaction.oncomplete = () => resolve()
    transaction.onabort = () =>
      reject(failure ?? storageError('FLOW_STORAGE_ABORTED', transaction.error))
    transaction.onerror = () => {
      failure ??= storageError('FLOW_STORAGE_WRITE_FAILED', transaction.error)
    }

    const abort = (error: StorageError): void => {
      failure ??= error
      try {
        transaction.abort()
      } catch {
        // An IndexedDB request error may already have started the abort. The
        // terminal `onabort` callback still rejects with the recorded cause.
      }
    }

    const validateAndWrite = (): void => {
      try {
        const expectedMatches =
          headTransition.expected !== null &&
          currentHead !== undefined &&
          sameHead(currentHead, headTransition.expected)
        const exactRetry =
          currentHead !== undefined && sameHead(currentHead, headTransition.resulting)
        const validateIdentities = (): void => {
          for (const group of populatedGroups) {
            assertIdentityCompatibility(existingByStore.get(group.storeName) ?? [], group.records)
          }
        }
        const newDocument = currentHead === undefined && headTransition.expected === null
        if (
          newDocument &&
          groups.some((group) => (existingByStore.get(group.storeName) ?? []).length > 0)
        ) {
          throw storageError('FLOW_STORE_PARTIAL_RECORD_SET')
        }
        if (exactRetry) {
          validateIdentities()
          if (!groups.every((group) => exactStoredGroup(existingByStore, group))) {
            throw storageError('FLOW_STORE_PARTIAL_RECORD_SET')
          }
        } else if (!expectedMatches && !newDocument) {
          throw storageError('FLOW_STORE_HEAD_CONFLICT')
        }
        if (expectedMatches) validateIdentities()
        for (const group of populatedGroups) {
          const objectStore = transaction.objectStore(group.storeName)
          for (const { envelope } of group.records) objectStore.put(envelope)
        }
        transaction.objectStore(HEADS).put(headTransition.resulting)
      } catch (error: unknown) {
        abort(
          error instanceof StorageError
            ? error
            : storageError('FLOW_STORAGE_WRITE_FAILED', error),
        )
      }
    }

    const ready = (): void => {
      readsRemaining -= 1
      if (readsRemaining === 0) validateAndWrite()
    }

    const metadataRequest = transaction.objectStore(METADATA).getAll()
    metadataRequest.onsuccess = () => {
      const metadata = metadataRequest.result as StorageMetadata[]
      if (metadata.some(({ key, value }) => key === LEGACY_MIGRATION_REQUIRED && value)) {
        abort(storageError('FLOW_STORAGE_MIGRATION_REQUIRED'))
      } else if (metadata.some(({ key, value }) => key === HEAD_BOOTSTRAP_REQUIRED && value)) {
        abort(storageError('FLOW_STORAGE_HEAD_REQUIRED'))
      } else {
        ready()
      }
    }
    metadataRequest.onerror = () =>
      abort(storageError('FLOW_STORAGE_READ_FAILED', metadataRequest.error))

    const headRequest = transaction
      .objectStore(HEADS)
      .get(headTransition.resulting.documentId)
    headRequest.onsuccess = () => {
      currentHead = headRequest.result as DocumentHeadDto | undefined
      ready()
    }
    headRequest.onerror = () =>
      abort(storageError('FLOW_STORAGE_READ_FAILED', headRequest.error))

    for (const group of groups) {
      const request = transaction.objectStore(group.storeName).getAll()
      request.onsuccess = () => {
        existingByStore.set(
          group.storeName,
          request.result as readonly StoredEnvelope<unknown>[],
        )
        ready()
      }
      request.onerror = () =>
        abort(storageError('FLOW_STORAGE_READ_FAILED', request.error))
    }
  })
}

function assertIdentityCompatibility(
  existing: readonly StoredEnvelope<unknown>[],
  candidates: readonly GuardedRecord[],
): void {
  const observed = [...existing]
  for (const candidate of candidates) {
    for (const durable of observed) {
      if (
        candidate.sameIdentity(durable.record, candidate.envelope.record) &&
        deterministicJson(durable.record) !== deterministicJson(candidate.envelope.record)
      ) {
        throw storageError('FLOW_STORE_IDENTITY_CONFLICT')
      }
    }
    observed.push(candidate.envelope)
  }
}

function exactStoredGroup(
  existingByStore: ReadonlyMap<string, readonly StoredEnvelope<unknown>[]>,
  group: GuardedStoreWrites,
): boolean {
  const expected = new Map<string, StoredEnvelope<unknown>>()
  for (const { envelope } of group.records) expected.set(envelope.physicalKey, envelope)
  const existing = existingByStore.get(group.storeName) ?? []
  return (
    existing.length === expected.size &&
    existing.every((durable) => {
      const candidate = expected.get(durable.physicalKey)
      return (
        candidate !== undefined &&
        deterministicJson(durable.record) === deterministicJson(candidate.record)
      )
    })
  )
}

function sameSnapshotIdentity(left: SnapshotRecordDto, right: SnapshotRecordDto): boolean {
  return (
    left.documentId === right.documentId &&
    left.schemaVersion === right.schemaVersion &&
    left.revision === right.revision
  )
}

function sameTransactionIdentity(
  left: TransactionRecordDto,
  right: TransactionRecordDto,
): boolean {
  return left.transactionId === right.transactionId
}

function sameAuditIdentity(left: AuditRecordDto, right: AuditRecordDto): boolean {
  return left.auditId === right.auditId
}

function sameAssetIdentity(left: AssetRecordDto, right: AssetRecordDto): boolean {
  return left.contentHash === right.contentHash
}

function sameMigrationSourceIdentity(
  left: MigrationSourceRecordDto,
  right: MigrationSourceRecordDto,
): boolean {
  return (
    left.documentId === right.documentId &&
    left.schemaVersion === right.schemaVersion &&
    left.canonicalHash === right.canonicalHash
  )
}

function sameHead(left: DocumentHeadDto, right: DocumentHeadDto): boolean {
  return (
    left.documentId === right.documentId &&
    left.revision === right.revision &&
    left.canonicalHash === right.canonicalHash
  )
}

function recoveryRecordsFromEnvelopes(
  stores: ReadonlyMap<string, readonly unknown[]>,
): RecoveryRecordsDto {
  const records = <T>(storeName: string): T[] =>
    (stores.get(storeName) ?? []).map(
      (value) => (value as StoredEnvelope<T>).record,
    )
  return {
    snapshots: records<SnapshotRecordDto>(SNAPSHOTS),
    transactions: records<TransactionRecordDto>(TRANSACTIONS),
    audits: records<AuditRecordDto>(AUDITS),
    assets: records<AssetRecordDto>(ASSETS),
    sources: records<MigrationSourceRecordDto>(SOURCES),
  }
}

function deterministicJson(value: unknown): string {
  const serialized = JSON.stringify(value, (_key, child: unknown) => {
    if (child === null || typeof child !== 'object' || Array.isArray(child)) return child

    const object = child as Record<string, unknown>
    return Object.fromEntries(
      Object.keys(object)
        .sort()
        .map((key) => [key, object[key]]),
    )
  })
  if (serialized === undefined) throw new TypeError('Record is not JSON serializable')
  return serialized
}

function requestResult<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(storageError('FLOW_STORAGE_READ_FAILED', request.error))
  })
}

function transactionComplete(transaction: IDBTransaction): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    transaction.oncomplete = () => resolve()
    transaction.onabort = () => reject(storageError('FLOW_STORAGE_ABORTED', transaction.error))
    transaction.onerror = () => {
      // See the matching write-side handler above.
    }
  })
}

export class StorageError extends Error {
  constructor(
    readonly code: string,
    options?: ErrorOptions,
  ) {
    super(code, options)
    this.name = 'StorageError'
  }
}

function storageError(code: string, cause?: unknown): StorageError {
  return new StorageError(code, cause === undefined ? undefined : { cause })
}
