export interface LogicalPositionDto {
  readonly nodeId: string
  readonly utf16Offset: number
  readonly affinity: 'forward' | 'backward'
}

export type AuditCommandKindDto =
  | 'insertText'
  | 'replaceText'
  | 'replaceSelection'
  | 'deleteText'
  | 'setNodeStyle'
  | 'insertNode'
  | 'deleteNode'
  | 'setField'
  | 'batch'
  | 'undo'
  | 'redo'

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

export interface RecoveryImageDto {
  readonly records: RecoveryRecordsDto
  readonly head: DocumentHeadDto | null
}

const DEFAULT_DATABASE_NAME = 'flowpdf-foundation'
const DATABASE_VERSION = 5
const RECORD_FORMAT_VERSION = 1
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
const RECOVERY_RECORD_LIMIT = 10_000
const RECOVERY_BYTE_LIMIT = 64 * 1024 * 1024

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

const ASSET_ENVELOPE_VERSION = 1

interface BinaryAssetRecordEnvelope {
  readonly envelopeVersion: typeof ASSET_ENVELOPE_VERSION
  readonly recordFormatVersion: number
  readonly contentHash: string
  readonly byteLength: number
  readonly bytes: Uint8Array
}

type SameIdentity<T> = (left: T, right: T) => boolean

interface GuardedRecord {
  readonly envelope: StoredEnvelope<unknown>
  readonly sameIdentity: SameIdentity<unknown>
  readonly logicalRecord: (record: unknown) => unknown
}

interface GuardedStoreWrites {
  readonly storeName: string
  readonly records: readonly GuardedRecord[]
  readonly logicalRecord?: (record: unknown) => unknown
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
      commit.assets.map((asset) => assetRecordEnvelope(asset, 'FLOW_STORAGE_WRITE_FAILED')),
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
          records: assets.map((asset) => assetGuardedRecord(asset)),
          logicalRecord: decodeStoredAssetRecord,
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
        commit.assets.map((asset) => assetRecordEnvelope(asset, 'FLOW_STORAGE_WRITE_FAILED')),
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
          records: assets.map((asset) => assetGuardedRecord(asset)),
          logicalRecord: decodeStoredAssetRecord,
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

  async commitStandaloneAudit(auditRecord: AuditRecordDto): Promise<void> {
    const audit = await recordEnvelope(auditRecord, 'FLOW_STORAGE_WRITE_FAILED')
    const database = await this.open()

    await new Promise<void>((resolve, reject) => {
      const transaction = database.transaction(
        [SNAPSHOTS, TRANSACTIONS, AUDITS, ASSETS, SOURCES, METADATA],
        'readwrite',
        { durability: 'strict' },
      )
      let snapshots: readonly StoredEnvelope<SnapshotRecordDto>[] = []
      let transactions: readonly StoredEnvelope<TransactionRecordDto>[] = []
      let audits: readonly StoredEnvelope<AuditRecordDto>[] = []
      let assets: readonly StoredEnvelope<unknown>[] = []
      let sources: readonly StoredEnvelope<MigrationSourceRecordDto>[] = []
      let metadata: readonly StorageMetadata[] = []
      let readsRemaining = 6
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
          // A failed request may already have started the transaction abort.
        }
      }
      const ready = (): void => {
        readsRemaining -= 1
        if (readsRemaining !== 0) return
        try {
          if (metadata.some(({ key, value }) => key === LEGACY_MIGRATION_REQUIRED && value)) {
            throw storageError('FLOW_STORAGE_MIGRATION_REQUIRED')
          }
          const isSupportedStandaloneAudit =
            (auditRecord.action.type === 'command' &&
              auditRecord.outcome.kind === 'failure') ||
            (auditRecord.action.type === 'recovery' &&
              (auditRecord.outcome.kind === 'success' ||
                auditRecord.outcome.kind === 'failure'))
          if (
            auditRecord.recordFormatVersion !== RECORD_FORMAT_VERSION ||
            auditRecord.transactionId !== auditRecord.commandId ||
            auditRecord.baseRevision !== auditRecord.newRevision ||
            !isSupportedStandaloneAudit
          ) {
            throw storageError('FLOW_STORE_INVALID_COMMIT')
          }
          if (
            (auditRecord.action.type === 'command' &&
              auditRecord.auditId === auditRecord.commandId) ||
            (auditRecord.action.type === 'recovery' &&
              auditRecord.auditId !== auditRecord.commandId)
          ) {
            throw storageError('FLOW_STORE_INVALID_COMMIT')
          }
          const hasRevisionAnchor =
            snapshots.some(
              ({ record }) =>
                record.documentId === auditRecord.documentId &&
                record.revision === auditRecord.newRevision,
            ) ||
            transactions.some(
              ({ record }) =>
                record.documentId === auditRecord.documentId &&
                record.newRevision === auditRecord.newRevision,
            )
          if (!hasRevisionAnchor) throw storageError('FLOW_STORE_INVALID_COMMIT')
          assertIdentityCompatibility(audits, [guardedRecord(audit, sameAuditIdentity)])
          assertRecoveryBudget(
            new Map<string, readonly StoredEnvelope<unknown>[]>([
              [SNAPSHOTS, snapshots],
              [TRANSACTIONS, transactions],
              [AUDITS, audits],
              [ASSETS, assets],
              [SOURCES, sources],
            ]),
            [
              { storeName: SNAPSHOTS, records: [] },
              { storeName: TRANSACTIONS, records: [] },
              { storeName: AUDITS, records: [guardedRecord(audit, sameAuditIdentity)] },
              { storeName: ASSETS, records: [], logicalRecord: decodeStoredAssetRecord },
              { storeName: SOURCES, records: [] },
            ],
          )
          transaction.objectStore(AUDITS).put(audit)
        } catch (error: unknown) {
          abort(
            error instanceof StorageError
              ? error
              : storageError('FLOW_STORAGE_WRITE_FAILED', error),
          )
        }
      }
      const read = <T>(storeName: string, assign: (records: readonly T[]) => void): void => {
        const request = transaction.objectStore(storeName).getAll()
        request.onsuccess = () => {
          assign(request.result as readonly T[])
          ready()
        }
        request.onerror = () =>
          abort(storageError('FLOW_STORAGE_READ_FAILED', request.error))
      }

      read<StoredEnvelope<SnapshotRecordDto>>(SNAPSHOTS, (records) => {
        snapshots = records
      })
      read<StoredEnvelope<TransactionRecordDto>>(TRANSACTIONS, (records) => {
        transactions = records
      })
      read<StoredEnvelope<AuditRecordDto>>(AUDITS, (records) => {
        audits = records
      })
      read<StoredEnvelope<unknown>>(ASSETS, (records) => {
        assets = records
      })
      read<StoredEnvelope<MigrationSourceRecordDto>>(SOURCES, (records) => {
        sources = records
      })
      read<StorageMetadata>(METADATA, (records) => {
        metadata = records
      })
    })
  }

  async loadRecords(options: { readonly allowEmpty?: boolean } = {}): Promise<RecoveryRecordsDto> {
    return (await this.loadRecoveryImage(options)).records
  }

  async loadRecoveryImage(
    options: { readonly allowEmpty?: boolean } = {},
  ): Promise<RecoveryImageDto> {
    const database = await this.open()
    const transaction = database.transaction(
      [SNAPSHOTS, TRANSACTIONS, AUDITS, ASSETS, SOURCES, METADATA, HEADS],
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
    const headsRequest = transaction.objectStore(HEADS).getAll()

    const [
      snapshotEnvelopes,
      transactionEnvelopes,
      auditEnvelopes,
      assetEnvelopes,
      sourceEnvelopes,
      migrationRequired,
      heads,
    ] = await Promise.all([
      requestResult<StoredEnvelope<SnapshotRecordDto>[]>(snapshotsRequest),
      requestResult<StoredEnvelope<TransactionRecordDto>[]>(transactionsRequest),
      requestResult<StoredEnvelope<AuditRecordDto>[]>(auditsRequest),
      requestResult<StoredEnvelope<unknown>[]>(assetsRequest),
      requestResult<StoredEnvelope<MigrationSourceRecordDto>[]>(sourcesRequest),
      requestResult<StorageMetadata | undefined>(migrationRequiredRequest),
      requestResult<DocumentHeadDto[]>(headsRequest),
      transactionComplete(transaction),
    ])
    const snapshots = snapshotEnvelopes.map(({ record }) => record)
    const transactions = transactionEnvelopes.map(({ record }) => record)
    const audits = auditEnvelopes.map(({ record }) => record)
    const assets = assetEnvelopes.map(({ record }) => decodeStoredAssetRecord(record))
    const sources = sourceEnvelopes.map(({ record }) => record)

    if (migrationRequired?.value === true) {
      throw storageError('FLOW_STORAGE_MIGRATION_REQUIRED')
    }
    if (heads.length > 1) throw storageError('FLOW_STORE_HEAD_CONFLICT')

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
      records: { snapshots, transactions, audits, assets, sources },
      head: heads[0] ?? null,
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
        database.onclose = () => {
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

async function assetRecordEnvelope(
  asset: AssetRecordDto,
  errorCode: string,
): Promise<StoredEnvelope<BinaryAssetRecordEnvelope>> {
  try {
    const bytes = new Uint8Array(asset.bytes)
    const logicalRecord: AssetRecordDto = {
      recordFormatVersion: asset.recordFormatVersion,
      contentHash: asset.contentHash,
      bytes: Array.from(bytes),
    }
    const digest = await globalThis.crypto.subtle.digest(
      'SHA-256',
      new TextEncoder().encode(deterministicJson(logicalRecord)),
    )
    const physicalKey = Array.from(new Uint8Array(digest), (byte) =>
      byte.toString(16).padStart(2, '0'),
    ).join('')
    return {
      physicalKey,
      record: {
        envelopeVersion: ASSET_ENVELOPE_VERSION,
        recordFormatVersion: asset.recordFormatVersion,
        contentHash: asset.contentHash,
        byteLength: bytes.byteLength,
        bytes,
      },
    }
  } catch (error: unknown) {
    throw storageError(errorCode, error)
  }
}

function guardedRecord<T>(
  envelope: StoredEnvelope<T>,
  sameIdentity: SameIdentity<T>,
  logicalRecord: (record: unknown) => unknown = (record) => record,
): GuardedRecord {
  return {
    envelope: envelope as StoredEnvelope<unknown>,
    sameIdentity: (left, right) => sameIdentity(left as T, right as T),
    logicalRecord,
  }
}

function assetGuardedRecord(
  envelope: StoredEnvelope<BinaryAssetRecordEnvelope>,
): GuardedRecord {
  return {
    envelope: envelope as StoredEnvelope<unknown>,
    sameIdentity: (left, right) =>
      sameAssetIdentity(decodeStoredAssetRecord(left), decodeStoredAssetRecord(right)),
    logicalRecord: decodeStoredAssetRecord,
  }
}

function decodeStoredAssetRecord(value: unknown): AssetRecordDto {
  try {
    if (isBinaryAssetRecordEnvelope(value)) {
      if (value.byteLength !== value.bytes.byteLength) {
        throw new TypeError('Binary asset byte length does not match its payload')
      }
      return {
        recordFormatVersion: value.recordFormatVersion,
        contentHash: value.contentHash,
        bytes: Array.from(value.bytes),
      }
    }
    if (!isLegacyAssetRecord(value)) throw new TypeError('Invalid asset record')
    return {
      recordFormatVersion: value.recordFormatVersion,
      contentHash: value.contentHash,
      bytes: [...value.bytes],
    }
  } catch (error: unknown) {
    if (error instanceof StorageError) throw error
    throw storageError('FLOW_STORAGE_READ_FAILED', error)
  }
}

function isBinaryAssetRecordEnvelope(
  value: unknown,
): value is BinaryAssetRecordEnvelope {
  if (value === null || typeof value !== 'object') return false
  const candidate = value as Partial<BinaryAssetRecordEnvelope>
  return (
    candidate.envelopeVersion === ASSET_ENVELOPE_VERSION &&
    typeof candidate.recordFormatVersion === 'number' &&
    typeof candidate.contentHash === 'string' &&
    typeof candidate.byteLength === 'number' &&
    candidate.bytes instanceof Uint8Array
  )
}

function isLegacyAssetRecord(
  value: unknown,
): value is AssetRecordDto {
  if (value === null || typeof value !== 'object') return false
  const candidate = value as Partial<AssetRecordDto>
  return (
    typeof candidate.recordFormatVersion === 'number' &&
    typeof candidate.contentHash === 'string' &&
    Array.isArray(candidate.bytes) &&
    candidate.bytes.every((byte) => Number.isInteger(byte) && byte >= 0 && byte <= 255)
  )
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
          // A core-planned explicit save may add the current revision's
          // checkpoint after its transaction and audit are already durable.
          // Creation and migration retries still require their snapshot to
          // have been part of the original atomic boundary.
          const requiredRetryGroups =
            headTransition.expected === null
              ? groups
              : groups.filter(({ storeName }) => storeName !== SNAPSHOTS)
          if (!requiredRetryGroups.every((group) => exactStoredGroup(existingByStore, group))) {
            throw storageError('FLOW_STORE_PARTIAL_RECORD_SET')
          }
        } else if (!expectedMatches && !newDocument) {
          throw storageError('FLOW_STORE_HEAD_CONFLICT')
        }
        if (expectedMatches) validateIdentities()
        assertRecoveryBudget(existingByStore, groups)
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

function assertRecoveryBudget(
  existingByStore: ReadonlyMap<string, readonly StoredEnvelope<unknown>[]>,
  groups: readonly GuardedStoreWrites[],
): void {
  let recordCount = 0
  let replayBytes = 0
  const encoder = new TextEncoder()
  for (const group of groups) {
    const projected = new Map(
      (existingByStore.get(group.storeName) ?? []).map((envelope) => [
        envelope.physicalKey,
        envelope,
      ]),
    )
    for (const { envelope } of group.records) projected.set(envelope.physicalKey, envelope)
    recordCount += projected.size
    for (const { record } of projected.values()) {
      replayBytes += encoder
        .encode(deterministicJson(group.logicalRecord?.(record) ?? record))
        .byteLength
    }
  }
  if (recordCount > RECOVERY_RECORD_LIMIT) {
    throw storageError('FLOW_LIMIT_RECOVERY_RECORDS')
  }
  if (replayBytes > RECOVERY_BYTE_LIMIT) {
    throw storageError('FLOW_LIMIT_RECOVERY_BYTES')
  }
}

function assertIdentityCompatibility(
  existing: readonly StoredEnvelope<unknown>[],
  candidates: readonly GuardedRecord[],
): void {
  const observed = [...existing]
  for (const candidate of candidates) {
    for (const durable of observed) {
      const durableLogical = candidate.logicalRecord(durable.record)
      const candidateLogical = candidate.logicalRecord(candidate.envelope.record)
      if (
        candidate.sameIdentity(durableLogical, candidateLogical) &&
        deterministicJson(durableLogical) !== deterministicJson(candidateLogical)
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
  const existing = existingByStore.get(group.storeName) ?? []
  return group.records.every(({ envelope: candidate }) =>
    existing.some(
      (durable) =>
        durable.physicalKey === candidate.physicalKey &&
        deterministicJson(
          group.records[0]?.logicalRecord(durable.record) ?? durable.record,
        ) ===
          deterministicJson(
            group.records[0]?.logicalRecord(candidate.record) ?? candidate.record,
          ),
    ),
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
    assets: (stores.get(ASSETS) ?? []).map((value) =>
      decodeStoredAssetRecord((value as StoredEnvelope<unknown>).record),
    ),
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
