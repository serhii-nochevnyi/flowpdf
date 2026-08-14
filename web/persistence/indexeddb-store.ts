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

const DATABASE_NAME = 'flowpdf-foundation'
const DATABASE_VERSION = 3
const SNAPSHOTS = `snapshots-v${DATABASE_VERSION}`
const TRANSACTIONS = `transactions-v${DATABASE_VERSION}`
const AUDITS = `audits-v${DATABASE_VERSION}`
const ASSETS = `assets-v${DATABASE_VERSION}`
const SOURCES = `migration-sources-v${DATABASE_VERSION}`

interface StoredEnvelope<T> {
  readonly physicalKey: string
  readonly record: T
}

export class IndexedDbDocumentStore {
  private databasePromise: Promise<IDBDatabase> | undefined

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

    await new Promise<void>((resolve, reject) => {
      const transaction = database.transaction(
        [SNAPSHOTS, TRANSACTIONS, AUDITS, ASSETS],
        'readwrite',
        { durability: 'strict' },
      )

      transaction.oncomplete = () => resolve()
      transaction.onabort = () => reject(storageError('FLOW_STORAGE_ABORTED', transaction.error))
      transaction.onerror = () => {
        // `abort` is the terminal event. Keeping this handler prevents an
        // implementation-specific uncaught error while still rejecting only
        // when the atomic transaction has definitively failed.
      }

      try {
        const snapshots = transaction.objectStore(SNAPSHOTS)
        const transactions = transaction.objectStore(TRANSACTIONS)
        const audits = transaction.objectStore(AUDITS)
        const assetStore = transaction.objectStore(ASSETS)
        // `replaceExisting` marks a Rust-validated document boundary. It must
        // never clear the whole physical database: an exact repeat is deduped
        // by Rust during recovery and a divergent identity must stay visible
        // so Rust can reject it rather than silently replacing durable truth.
        if (snapshot !== null) snapshots.put(snapshot)
        transactions.put(transactionRecord)
        audits.put(audit)
        for (const asset of assets) assetStore.put(asset)
      } catch (error: unknown) {
        transaction.abort()
        reject(storageError('FLOW_STORAGE_WRITE_FAILED', error))
      }
    })
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

    await new Promise<void>((resolve, reject) => {
      const transaction = database.transaction(
        [SNAPSHOTS, AUDITS, ASSETS, SOURCES],
        'readwrite',
        { durability: 'strict' },
      )

      transaction.oncomplete = () => resolve()
      transaction.onabort = () => reject(storageError('FLOW_STORAGE_ABORTED', transaction.error))
      transaction.onerror = () => {
        // `abort` is the terminal event; see the normal commit path.
      }

      try {
        transaction.objectStore(SNAPSHOTS).put(snapshot)
        transaction.objectStore(AUDITS).put(audit)
        const assetStore = transaction.objectStore(ASSETS)
        for (const asset of assets) assetStore.put(asset)
        transaction.objectStore(SOURCES).put(source)
      } catch (error: unknown) {
        transaction.abort()
        reject(storageError('FLOW_STORAGE_WRITE_FAILED', error))
      }
    })
  }

  async loadRecords(options: { readonly allowEmpty?: boolean } = {}): Promise<RecoveryRecordsDto> {
    const database = await this.open()
    const transaction = database.transaction(
      [SNAPSHOTS, TRANSACTIONS, AUDITS, ASSETS, SOURCES],
      'readonly',
    )
    const snapshotsRequest = transaction.objectStore(SNAPSHOTS).getAll()
    const transactionsRequest = transaction.objectStore(TRANSACTIONS).getAll()
    const auditsRequest = transaction.objectStore(AUDITS).getAll()
    const assetsRequest = transaction.objectStore(ASSETS).getAll()
    const sourcesRequest = transaction.objectStore(SOURCES).getAll()

    const [
      snapshotEnvelopes,
      transactionEnvelopes,
      auditEnvelopes,
      assetEnvelopes,
      sourceEnvelopes,
    ] = await Promise.all([
      requestResult<StoredEnvelope<SnapshotRecordDto>[]>(snapshotsRequest),
      requestResult<StoredEnvelope<TransactionRecordDto>[]>(transactionsRequest),
      requestResult<StoredEnvelope<AuditRecordDto>[]>(auditsRequest),
      requestResult<StoredEnvelope<AssetRecordDto>[]>(assetsRequest),
      requestResult<StoredEnvelope<MigrationSourceRecordDto>[]>(sourcesRequest),
      transactionComplete(transaction),
    ])
    const snapshots = snapshotEnvelopes.map(({ record }) => record)
    const transactions = transactionEnvelopes.map(({ record }) => record)
    const audits = auditEnvelopes.map(({ record }) => record)
    const assets = assetEnvelopes.map(({ record }) => record)
    const sources = sourceEnvelopes.map(({ record }) => record)

    if (snapshots.length === 0 && options.allowEmpty !== true) {
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

  private open(): Promise<IDBDatabase> {
    this.databasePromise ??= new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open(DATABASE_NAME, DATABASE_VERSION)
      request.onupgradeneeded = () => {
        const database = request.result
        for (const storeName of [SNAPSHOTS, TRANSACTIONS, AUDITS, ASSETS, SOURCES]) {
          if (!database.objectStoreNames.contains(storeName)) {
            database.createObjectStore(storeName, { keyPath: 'physicalKey' })
          }
        }
      }
      request.onsuccess = () => resolve(request.result)
      request.onerror = () => reject(storageError('FLOW_STORAGE_OPEN_FAILED', request.error))
      request.onblocked = () => reject(storageError('FLOW_STORAGE_BLOCKED'))
    })
    return this.databasePromise
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
