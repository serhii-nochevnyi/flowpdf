export interface LogicalPositionDto {
  readonly nodeId: string
  readonly utf16Offset: number
  readonly affinity: 'forward' | 'backward'
}

export interface SafeMetadataDto {
  readonly key: string
  readonly value: string
}

export interface AuditRecordDto {
  readonly auditId: string
  readonly documentId: string
  readonly commandId: string
  readonly baseRevision: number
  readonly newRevision: number
  readonly timestamp: string
  readonly commandType: string
  readonly modality: 'ui' | 'keyboard' | 'voice' | 'api' | 'system'
  readonly outcome: 'applied'
  readonly errorCode: string | null
  readonly safeMetadata: readonly SafeMetadataDto[]
}

export interface SnapshotRecordDto {
  readonly documentId: string
  readonly revision: number
  readonly schemaVersion: number
  readonly canonicalJson: string
  readonly canonicalHash: string
}

export interface TransactionRecordDto {
  readonly transactionId: string
  readonly documentId: string
  readonly commandId: string
  readonly baseRevision: number
  readonly newRevision: number
  readonly commandType: string
  readonly modality: 'ui' | 'keyboard' | 'voice' | 'api' | 'system'
  readonly beforeHash: string
  readonly afterHash: string
  readonly forwardOperations: readonly unknown[]
  readonly inverseOperations: readonly unknown[]
}

export interface PersistenceCommitDto {
  readonly replaceExisting: boolean
  readonly snapshot: SnapshotRecordDto
  readonly transaction: TransactionRecordDto
  readonly audit: AuditRecordDto
}

export interface RecoveryRecordsDto {
  readonly snapshot: SnapshotRecordDto
  readonly transactions: readonly TransactionRecordDto[]
  readonly audits: readonly AuditRecordDto[]
}

const DATABASE_NAME = 'flowpdf-foundation'
const DATABASE_VERSION = 1
const SNAPSHOTS = 'snapshots'
const TRANSACTIONS = 'transactions'
const AUDITS = 'audits'

export class IndexedDbDocumentStore {
  private databasePromise: Promise<IDBDatabase> | undefined

  async commit(commit: PersistenceCommitDto): Promise<void> {
    const database = await this.open()

    await new Promise<void>((resolve, reject) => {
      const transaction = database.transaction(
        [SNAPSHOTS, TRANSACTIONS, AUDITS],
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
        if (commit.replaceExisting) {
          snapshots.clear()
          transactions.clear()
          audits.clear()
        }
        snapshots.put(commit.snapshot)
        transactions.put(commit.transaction)
        audits.put(commit.audit)
      } catch (error: unknown) {
        transaction.abort()
        reject(storageError('FLOW_STORAGE_WRITE_FAILED', error))
      }
    })
  }

  async loadLatest(): Promise<RecoveryRecordsDto> {
    const database = await this.open()
    const transaction = database.transaction(
      [SNAPSHOTS, TRANSACTIONS, AUDITS],
      'readonly',
    )
    const snapshotsRequest = transaction.objectStore(SNAPSHOTS).getAll()
    const transactionsRequest = transaction.objectStore(TRANSACTIONS).getAll()
    const auditsRequest = transaction.objectStore(AUDITS).getAll()

    const [snapshots, transactions, audits] = await Promise.all([
      requestResult<SnapshotRecordDto[]>(snapshotsRequest),
      requestResult<TransactionRecordDto[]>(transactionsRequest),
      requestResult<AuditRecordDto[]>(auditsRequest),
      transactionComplete(transaction),
    ])

    const snapshot = snapshots
      .slice()
      .sort((left, right) => right.revision - left.revision)[0]
    if (snapshot === undefined) {
      throw storageError('FLOW_STORAGE_EMPTY')
    }

    return {
      snapshot,
      transactions: transactions
        .filter((record) => record.documentId === snapshot.documentId)
        .sort((left, right) => left.newRevision - right.newRevision),
      audits: audits
        .filter((record) => record.documentId === snapshot.documentId)
        .sort((left, right) => left.newRevision - right.newRevision),
    }
  }

  private open(): Promise<IDBDatabase> {
    this.databasePromise ??= new Promise<IDBDatabase>((resolve, reject) => {
      const request = indexedDB.open(DATABASE_NAME, DATABASE_VERSION)
      request.onupgradeneeded = () => {
        const database = request.result
        if (!database.objectStoreNames.contains(SNAPSHOTS)) {
          database.createObjectStore(SNAPSHOTS, { keyPath: 'documentId' })
        }
        if (!database.objectStoreNames.contains(TRANSACTIONS)) {
          database.createObjectStore(TRANSACTIONS, { keyPath: 'transactionId' })
        }
        if (!database.objectStoreNames.contains(AUDITS)) {
          database.createObjectStore(AUDITS, { keyPath: 'auditId' })
        }
      }
      request.onsuccess = () => resolve(request.result)
      request.onerror = () => reject(storageError('FLOW_STORAGE_OPEN_FAILED', request.error))
      request.onblocked = () => reject(storageError('FLOW_STORAGE_BLOCKED'))
    })
    return this.databasePromise
  }
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
