import { IDBFactory, IDBObjectStore, forceCloseDatabase } from 'fake-indexeddb'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  IndexedDbDocumentStore,
  type AuditRecordDto,
  type MigrationPersistenceCommitDto,
  type PersistenceCommitDto,
  type PlannedPersistenceCommitDto,
} from '../persistence/indexeddb-store.js'

const databaseName = 'flowpdf-foundation'
const documentId = 'document-1'

function commitFor(revision: number, assetId: string): PersistenceCommitDto {
  return {
    replaceExisting: false,
    snapshot: {
      recordFormatVersion: 1,
      documentId,
      revision,
      schemaVersion: 1,
      canonicalJson: `{"documentId":"${documentId}","revision":${revision},"blocks":[]}`,
      canonicalHash: `snapshot-${revision}`,
      history: {
        entries: [],
        cursor: 0,
        seenCommandIds: [],
      },
    },
    transaction: {
      recordFormatVersion: 1,
      documentId,
      transactionId: `transaction-${revision}`,
      commandId: `command-${revision}`,
      schemaVersion: 1,
      baseRevision: revision - 1,
      newRevision: revision,
      commandType: 'insert_text',
      modality: 'keyboard',
      issuedAt: `2026-08-15T00:00:0${revision}Z`,
      beforeHash: `snapshot-${revision - 1}`,
      afterHash: `snapshot-${revision}`,
      forwardOperations: [],
      inverseOperations: [],
      anchorMapping: { segments: [] },
      historyEffect: { type: 'commit' },
    },
    audit: {
      recordFormatVersion: 1,
      auditId: `audit-${revision}`,
      documentId,
      transactionId: `transaction-${revision}`,
      commandId: `command-${revision}`,
      baseRevision: revision - 1,
      newRevision: revision,
      durableSequence: revision,
      timestamp: `2026-08-15T00:00:0${revision}Z`,
      action: { type: 'command', commandKind: 'insertText' },
      modality: 'keyboard',
      outcome: { kind: 'success' },
      metadata: [{ kind: 'schemaVersion', value: 1 }],
    },
    assets: [
      {
        recordFormatVersion: 1,
        contentHash: assetId,
        bytes: [revision, revision + 1],
      },
    ],
  }
}

function failedCommandAuditAt(
  commit: PersistenceCommitDto,
  auditId = `failed-command-attempt-${commit.snapshot.revision}`,
): AuditRecordDto {
  const commandId = `failed-command-${commit.snapshot.revision}`
  return {
    ...commit.audit,
    auditId,
    transactionId: commandId,
    commandId,
    baseRevision: commit.snapshot.revision,
    newRevision: commit.snapshot.revision,
    durableSequence: commit.snapshot.revision + 1,
    action: { type: 'command', commandKind: 'insertText' },
    outcome: { kind: 'failure', code: 'FLOW_STALE_REVISION' },
  }
}

function migrationCommit(): MigrationPersistenceCommitDto {
  const candidate = commitFor(1, 'migration-asset')
  return {
    snapshot: candidate.snapshot,
    audit: {
      ...candidate.audit,
      auditId: 'migration-audit',
      action: { type: 'migration' },
      modality: 'system',
      metadata: [{ kind: 'migration', fromSchemaVersion: 0, toSchemaVersion: 1 }],
    },
    assets: candidate.assets,
    source: {
      recordFormatVersion: 1,
      documentId,
      schemaVersion: 0,
      canonicalJson: `{"documentId":"${documentId}","schemaVersion":0}`,
      canonicalHash: 'legacy-source-hash',
    },
  }
}

describe('IndexedDbDocumentStore', () => {
  beforeEach(() => {
    // A fresh in-memory factory prevents a previous test's open connection from
    // blocking an upgrade and models a fresh browser storage partition.
    globalThis.indexedDB = new IDBFactory() as unknown as IDBFactory
  })

  afterEach(() => {
    vi.restoreAllMocks()
  })

  it('returns raw stored arrays without selecting or interpreting semantic records', async () => {
    const store = new IndexedDbDocumentStore()
    const first = commitFor(1, 'asset-1')
    const second = commitFor(2, 'asset-2')

    await store.commit(first)
    await store.commit(second)

    const records = await store.loadRecords()
    expect(records.snapshots).toHaveLength(2)
    expect(records.snapshots).toEqual(expect.arrayContaining([first.snapshot, second.snapshot]))
    expect(records.transactions).toHaveLength(2)
    expect(records.transactions).toEqual(
      expect.arrayContaining([first.transaction, second.transaction]),
    )
    expect(records.audits).toHaveLength(2)
    expect(records.audits).toEqual(expect.arrayContaining([first.audit, second.audit]))
    expect(records.assets).toHaveLength(2)
    expect(records.assets).toEqual(expect.arrayContaining([first.assets[0], second.assets[0]]))
    expect(records.sources).toEqual([])
  })

  it('stores asset payloads as versioned binary envelopes while exposing legacy DTOs', async () => {
    const store = new IndexedDbDocumentStore()
    const commit = commitFor(1, 'asset-binary')

    await store.commit(commit)

    const database = await openCurrentDatabase()
    const transaction = database.transaction('assets-v3', 'readonly')
    const stored = await requestResultForTest<{ readonly record: unknown }[]>(
      transaction.objectStore('assets-v3').getAll(),
    )
    await transactionCompleteForTest(transaction)
    database.close()

    expect(stored).toHaveLength(1)
    const envelope = stored[0]?.record as {
      readonly envelopeVersion: number
      readonly byteLength: number
      readonly bytes: Uint8Array
    }
    expect(envelope.envelopeVersion).toBe(1)
    expect(envelope.byteLength).toBe(commit.assets[0]!.bytes.length)
    expect(envelope.bytes).toBeInstanceOf(Uint8Array)
    expect(Array.isArray(envelope.bytes)).toBe(false)
    await expect(store.loadRecords()).resolves.toMatchObject({ assets: commit.assets })
  })

  it('reads v4 number-array asset records without changing their public DTO', async () => {
    const commit = commitFor(1, 'legacy-array-asset')
    const legacyDatabase = await openV4Database(commit)
    legacyDatabase.close()

    const store = new IndexedDbDocumentStore()
    await expect(store.loadRecords()).resolves.toMatchObject({ assets: commit.assets })
    await expect(store.loadRecords()).resolves.toMatchObject({ assets: commit.assets })
  })

  it('aborts the entire commit when the late assets write fails', async () => {
    const store = new IndexedDbDocumentStore()
    const baseline = commitFor(1, 'asset-1')
    const failed = commitFor(2, 'asset-2')

    await store.commit(baseline)

    const putOrder: string[] = []
    const originalPut = IDBObjectStore.prototype.put
    vi.spyOn(IDBObjectStore.prototype, 'put').mockImplementation(function put(
      this: IDBObjectStore,
      value,
    ) {
      putOrder.push(this.name)
      if (this.name === 'assets-v3') {
        throw new DOMException('forced late write failure', 'ConstraintError')
      }
      return originalPut.call(this, value)
    })

    await expect(store.commit(failed)).rejects.toMatchObject({
      code: 'FLOW_STORAGE_WRITE_FAILED',
    })
    expect(putOrder).toEqual(['snapshots-v3', 'transactions-v3', 'audits-v3', 'assets-v3'])
    await expect(store.loadRecords()).resolves.toEqual({
      snapshots: [baseline.snapshot],
      transactions: [baseline.transaction],
      audits: [baseline.audit],
      assets: [baseline.assets[0]],
      sources: [],
    })
  })

  it('deduplicates an exact physical record in each object store', async () => {
    const store = new IndexedDbDocumentStore()
    const duplicate = commitFor(1, 'asset-1')

    await store.commit(duplicate)
    await store.commit(duplicate)

    const records = await store.loadRecords()
    expect(records.snapshots).toEqual([duplicate.snapshot])
    expect(records.transactions).toEqual([duplicate.transaction])
    expect(records.audits).toEqual([duplicate.audit])
    expect(records.assets).toEqual([duplicate.assets[0]])
  })

  it('commits an authorized standalone failure audit idempotently without advancing the head', async () => {
    const store = new IndexedDbDocumentStore()
    const baseline = commitFor(1, 'asset-1')
    const failure = failedCommandAuditAt(baseline)

    await store.commit(baseline)
    await store.commitStandaloneAudit(failure)
    await store.commitStandaloneAudit(failure)

    await expect(store.loadRecords()).resolves.toMatchObject({
      snapshots: [baseline.snapshot],
      transactions: [baseline.transaction],
      audits: expect.arrayContaining([baseline.audit, failure]),
    })
    await expect(
      store.commitStandaloneAudit({
        ...failure,
        timestamp: '2026-08-15T00:00:59Z',
      }),
    ).rejects.toMatchObject({ code: 'FLOW_STORE_IDENTITY_CONFLICT' })
    await expect(
      store.commitStandaloneAudit({
        ...failure,
        auditId: 'unanchored-failure-attempt',
        transactionId: 'unanchored-failure-command',
        commandId: 'unanchored-failure-command',
        baseRevision: 99,
        newRevision: 99,
      }),
    ).rejects.toMatchObject({ code: 'FLOW_STORE_INVALID_COMMIT' })

    const next = commitFor(2, 'asset-2')
    await expect(store.commit({ ...next, snapshot: null })).resolves.toBeUndefined()
    expect((await store.loadRecords()).snapshots).toHaveLength(1)
    await expect(store.commit(next)).resolves.toBeUndefined()
    const saved = await store.loadRecords()
    expect(saved.transactions).toHaveLength(2)
    expect(saved.snapshots).toHaveLength(2)
  })

  it('rejects an audit-only write when its atomic post-image exceeds the recovery record limit', async () => {
    const store = new IndexedDbDocumentStore()
    const baseline = commitFor(1, 'asset-1')
    await store.commit(baseline)

    await injectRawRecords(
      Array.from({ length: 9_996 }, (_, index) => ({
        storeName: 'assets-v3',
        physicalKey: `budget-asset-${index}`,
        record: {
          recordFormatVersion: 1,
          contentHash: `budget-hash-${index}`,
          bytes: [],
        },
      })),
    )

    await expect(
      store.commitStandaloneAudit(failedCommandAuditAt(baseline)),
    ).rejects.toMatchObject({ code: 'FLOW_LIMIT_RECOVERY_RECORDS' })
    expect((await store.loadRecords()).audits).toEqual([baseline.audit])
  })

  it('rejects every divergent logical record identity without partial writes', async () => {
    const store = new IndexedDbDocumentStore()
    const baseline = commitFor(1, 'asset-1')
    const conflicts: readonly PersistenceCommitDto[] = [
      {
        ...baseline,
        snapshot: {
          ...baseline.snapshot,
          canonicalJson: `{"documentId":"${documentId}","revision":1,"blocks":["divergent"]}`,
          canonicalHash: 'divergent-snapshot-hash',
        },
      },
      {
        ...baseline,
        transaction: {
          ...baseline.transaction,
          issuedAt: '2026-08-15T00:00:59Z',
        },
      },
      {
        ...baseline,
        audit: {
          ...baseline.audit,
          timestamp: '2026-08-15T00:00:59Z',
        },
      },
      {
        ...baseline,
        assets: [{ ...baseline.assets[0]!, bytes: [99, 100] }],
      },
    ]

    await store.commit(baseline)
    for (const conflict of conflicts) {
      await expect(store.commit(conflict)).rejects.toMatchObject({
        code: 'FLOW_STORE_IDENTITY_CONFLICT',
      })
    }

    await expect(store.loadRecords()).resolves.toEqual({
      snapshots: [baseline.snapshot],
      transactions: [baseline.transaction],
      audits: [baseline.audit],
      assets: [baseline.assets[0]],
      sources: [],
    })
  })

  it('atomically admits exactly one of two overlapping divergent commits', async () => {
    const firstStore = new IndexedDbDocumentStore()
    const secondStore = new IndexedDbDocumentStore()
    const first = commitFor(1, 'asset-1')
    const second: PersistenceCommitDto = {
      ...first,
      transaction: {
        ...first.transaction,
        issuedAt: '2026-08-15T00:00:59Z',
      },
    }

    const outcomes = await Promise.allSettled([
      firstStore.commit(first),
      secondStore.commit(second),
    ])
    expect(outcomes.filter(({ status }) => status === 'fulfilled')).toHaveLength(1)
    const rejected = outcomes.find(({ status }) => status === 'rejected')
    expect(rejected).toMatchObject({
      status: 'rejected',
      reason: { code: 'FLOW_STORE_IDENTITY_CONFLICT' },
    })

    const winner = outcomes[0]?.status === 'fulfilled' ? first : second
    await expect(firstStore.loadRecords()).resolves.toEqual({
      snapshots: [winner.snapshot],
      transactions: [winner.transaction],
      audits: [winner.audit],
      assets: [winner.assets[0]],
      sources: [],
    })
  })

  it('allows overlapping exact retries to converge on one physical record set', async () => {
    const firstStore = new IndexedDbDocumentStore()
    const secondStore = new IndexedDbDocumentStore()
    const repeated = commitFor(1, 'asset-1')

    await expect(
      Promise.all([firstStore.commit(repeated), secondStore.commit(repeated)]),
    ).resolves.toEqual([undefined, undefined])

    await expect(firstStore.loadRecords()).resolves.toEqual({
      snapshots: [repeated.snapshot],
      transactions: [repeated.transaction],
      audits: [repeated.audit],
      assets: [repeated.assets[0]],
      sources: [],
    })
  })

  it('uses an atomic document-head CAS for divergent commits from the same base', async () => {
    const bootstrap = new IndexedDbDocumentStore()
    await bootstrap.commit(commitFor(1, 'asset-1'))

    const firstStore = new IndexedDbDocumentStore()
    const secondStore = new IndexedDbDocumentStore()
    const first = commitFor(2, 'asset-2')
    const second: PersistenceCommitDto = {
      ...first,
      snapshot: {
        ...first.snapshot,
        canonicalJson: `{"documentId":"${documentId}","revision":2,"blocks":["divergent"]}`,
        canonicalHash: 'snapshot-2-divergent',
      },
      transaction: {
        ...first.transaction,
        transactionId: 'transaction-2-divergent',
        commandId: 'command-2-divergent',
        afterHash: 'snapshot-2-divergent',
      },
      audit: {
        ...first.audit,
        auditId: 'audit-2-divergent',
        transactionId: 'transaction-2-divergent',
        commandId: 'command-2-divergent',
      },
      assets: [{ ...first.assets[0]!, contentHash: 'asset-2-divergent' }],
    }

    const outcomes = await Promise.allSettled([
      firstStore.commit(first),
      secondStore.commit(second),
    ])
    expect(outcomes.filter(({ status }) => status === 'fulfilled')).toHaveLength(1)
    expect(outcomes.find(({ status }) => status === 'rejected')).toMatchObject({
      status: 'rejected',
      reason: { code: 'FLOW_STORE_HEAD_CONFLICT' },
    })

    const winner = outcomes[0]?.status === 'fulfilled' ? first : second
    const records = await firstStore.loadRecords()
    expect(records.snapshots).toHaveLength(2)
    expect(records.snapshots).toEqual(
      expect.arrayContaining([commitFor(1, 'asset-1').snapshot, winner.snapshot]),
    )
    expect(records.transactions).toHaveLength(2)
    expect(records.transactions).toEqual(
      expect.arrayContaining([commitFor(1, 'asset-1').transaction, winner.transaction]),
    )
    expect(records.audits).toHaveLength(2)
    expect(records.audits).toEqual(
      expect.arrayContaining([commitFor(1, 'asset-1').audit, winner.audit]),
    )
  })

  it('bootstraps a head for existing v3 data only after the exact record image is recovered', async () => {
    const baseline = commitFor(1, 'asset-1')
    const previousDatabase = await openV4Database(baseline)
    previousDatabase.close()
    const store = new IndexedDbDocumentStore()
    const records = await store.loadRecords()

    await expect(store.commit(commitFor(2, 'asset-2'))).rejects.toMatchObject({
      code: 'FLOW_STORAGE_HEAD_REQUIRED',
    })
    await expect(
      store.installRecoveredHead(
        { ...records, audits: [] },
        { documentId, revision: 1, canonicalHash: 'snapshot-1' },
      ),
    ).rejects.toMatchObject({ code: 'FLOW_STORAGE_HEAD_REQUIRED' })

    await store.installRecoveredHead(records, {
      documentId,
      revision: 1,
      canonicalHash: 'snapshot-1',
    })
    await expect(store.commit(commitFor(2, 'asset-2'))).resolves.toBeUndefined()
  })

  it('keeps deliberately injected divergent opaque records visible for Rust validation', async () => {
    const store = new IndexedDbDocumentStore()
    const original = commitFor(1, 'asset-1')
    const candidate = commitFor(2, 'asset-2')
    const divergent: PersistenceCommitDto = {
      ...candidate,
      transaction: {
        ...candidate.transaction,
        transactionId: original.transaction.transactionId,
        commandId: original.transaction.commandId,
      },
      audit: {
        ...candidate.audit,
        auditId: original.audit.auditId,
        transactionId: original.audit.transactionId,
        commandId: original.audit.commandId,
      },
    }

    await store.commit(original)
    await injectRawRecords([
      {
        storeName: 'snapshots-v3',
        physicalKey: 'injected-divergent-snapshot',
        record: divergent.snapshot,
      },
      {
        storeName: 'transactions-v3',
        physicalKey: 'injected-divergent-transaction',
        record: divergent.transaction,
      },
      {
        storeName: 'audits-v3',
        physicalKey: 'injected-divergent-audit',
        record: divergent.audit,
      },
      {
        storeName: 'assets-v3',
        physicalKey: 'injected-divergent-asset',
        record: divergent.assets[0],
      },
    ])

    const records = await store.loadRecords()
    expect(records.transactions).toHaveLength(2)
    expect(records.transactions).toEqual(
      expect.arrayContaining([original.transaction, divergent.transaction]),
    )
    expect(records.audits).toHaveLength(2)
    expect(records.audits).toEqual(expect.arrayContaining([original.audit, divergent.audit]))
    expect(records.snapshots).toHaveLength(2)
    expect(records.snapshots).toEqual(
      expect.arrayContaining([original.snapshot, divergent.snapshot]),
    )
    expect(records.assets).toHaveLength(2)
    expect(records.assets).toEqual(
      expect.arrayContaining([original.assets[0], divergent.assets[0]]),
    )
  })

  it('stores a planned transaction without writing its optional snapshot', async () => {
    const store = new IndexedDbDocumentStore()
    const baseline = commitFor(1, 'asset-1')
    const candidate = commitFor(2, 'asset-2')
    const planned: PlannedPersistenceCommitDto = { ...candidate, snapshot: null }

    await store.commit(baseline)
    await store.commit(planned)

    const records = await store.loadRecords()
    expect(records.snapshots).toEqual([baseline.snapshot])
    expect(records.transactions).toHaveLength(2)
    expect(records.transactions).toEqual(
      expect.arrayContaining([baseline.transaction, candidate.transaction]),
    )
    expect(records.audits).toHaveLength(2)
    expect(records.assets).toHaveLength(2)
  })

  it('persists migration source bytes in the same atomic boundary', async () => {
    const store = new IndexedDbDocumentStore()
    const migration = migrationCommit()

    await store.commitMigration(migration)
    await store.commitMigration(migration)

    await expect(store.loadRecords()).resolves.toEqual({
      snapshots: [migration.snapshot],
      transactions: [],
      audits: [migration.audit],
      assets: [migration.assets[0]],
      sources: [migration.source],
    })
  })

  it('atomically admits exactly one overlapping migration with a divergent source identity', async () => {
    const firstStore = new IndexedDbDocumentStore()
    const secondStore = new IndexedDbDocumentStore()
    const first = migrationCommit()
    const second: MigrationPersistenceCommitDto = {
      ...first,
      source: {
        ...first.source,
        canonicalJson: `{"documentId":"${documentId}","schemaVersion":0,"divergent":true}`,
      },
    }

    const outcomes = await Promise.allSettled([
      firstStore.commitMigration(first),
      secondStore.commitMigration(second),
    ])
    expect(outcomes.filter(({ status }) => status === 'fulfilled')).toHaveLength(1)
    const rejected = outcomes.find(({ status }) => status === 'rejected')
    expect(rejected).toMatchObject({
      status: 'rejected',
      reason: { code: 'FLOW_STORE_IDENTITY_CONFLICT' },
    })

    const winner = outcomes[0]?.status === 'fulfilled' ? first : second
    await expect(firstStore.loadRecords()).resolves.toEqual({
      snapshots: [winner.snapshot],
      transactions: [],
      audits: [winner.audit],
      assets: [winner.assets[0]],
      sources: [winner.source],
    })
  })

  it('rejects an empty current store by default and exposes it only when requested', async () => {
    const store = new IndexedDbDocumentStore()

    await expect(store.loadRecords()).rejects.toMatchObject({ code: 'FLOW_STORAGE_EMPTY' })
    await expect(store.loadRecords({ allowEmpty: true })).resolves.toEqual({
      snapshots: [],
      transactions: [],
      audits: [],
      assets: [],
      sources: [],
    })
  })

  it('keeps partial creation sets visible and refuses to reconstruct missing records', async () => {
    for (const missingStore of [
      'snapshots-v3',
      'transactions-v3',
      'audits-v3',
      'assets-v3',
    ]) {
      globalThis.indexedDB = new IDBFactory() as unknown as IDBFactory
      const store = new IndexedDbDocumentStore()
      const creation = commitFor(1, 'asset-1')
      await store.commit(creation)

      const database = await openCurrentDatabase()
      const transaction = database.transaction(missingStore, 'readwrite')
      transaction.objectStore(missingStore).clear()
      await transactionCompleteForTest(transaction)
      database.close()

      await expect(store.loadRecords()).resolves.toBeDefined()
      await expect(store.commit(creation)).rejects.toMatchObject({
        code: 'FLOW_STORE_PARTIAL_RECORD_SET',
      })
      const after = await store.loadRecords({ allowEmpty: true })
      const missingCollection = {
        'snapshots-v3': after.snapshots,
        'transactions-v3': after.transactions,
        'audits-v3': after.audits,
        'assets-v3': after.assets,
      }[missingStore]
      expect(missingCollection).toEqual([])
    }
  })

  it('refuses to reconstruct every missing member of a migration record set', async () => {
    for (const missingStore of [
      'snapshots-v3',
      'audits-v3',
      'assets-v3',
      'migration-sources-v3',
    ]) {
      globalThis.indexedDB = new IDBFactory() as unknown as IDBFactory
      const store = new IndexedDbDocumentStore()
      const migration = migrationCommit()
      await store.commitMigration(migration)

      const database = await openCurrentDatabase()
      const transaction = database.transaction(missingStore, 'readwrite')
      transaction.objectStore(missingStore).clear()
      await transactionCompleteForTest(transaction)
      database.close()

      await expect(store.commitMigration(migration)).rejects.toMatchObject({
        code: 'FLOW_STORE_PARTIAL_RECORD_SET',
      })
    }
  })

  it('retries after a transient open failure and closes cached connections on version change', async () => {
    const open = vi.spyOn(globalThis.indexedDB, 'open')
    open.mockImplementationOnce(() => {
      throw new DOMException('transient open failure', 'UnknownError')
    })
    const store = new IndexedDbDocumentStore()

    await expect(store.loadRecords({ allowEmpty: true })).rejects.toMatchObject({
      code: 'FLOW_STORAGE_OPEN_FAILED',
    })
    await expect(store.loadRecords({ allowEmpty: true })).resolves.toMatchObject({
      snapshots: [],
    })

    await deleteDatabaseForTest(databaseName)
    await expect(store.loadRecords({ allowEmpty: true })).resolves.toMatchObject({
      snapshots: [],
    })
    expect(open).toHaveBeenCalledTimes(3)
  })

  it('reopens after an abnormal IndexedDB close instead of retaining a poisoned cache', async () => {
    const open = vi.spyOn(globalThis.indexedDB, 'open')
    const store = new IndexedDbDocumentStore()
    await store.loadRecords({ allowEmpty: true })
    const firstRequest = open.mock.results[0]?.value as IDBOpenDBRequest

    forceCloseDatabase(
      firstRequest.result as unknown as Parameters<typeof forceCloseDatabase>[0],
    )

    await expect(store.loadRecords({ allowEmpty: true })).resolves.toMatchObject({
      snapshots: [],
    })
    expect(open).toHaveBeenCalledTimes(2)
  })

  it('preserves incompatible v2 records and blocks v3 reads and writes pending migration', async () => {
    const legacyRecords = {
      snapshots: [{ legacy: true, revision: 2, canonicalHash: 'legacy-hash' }],
      transactions: [{ legacy: true, revision: 2 }],
      audits: [{ legacy: true, revision: 2 }],
    }
    const legacyDatabase = await openLegacyDatabase(legacyRecords)
    legacyDatabase.close()

    const store = new IndexedDbDocumentStore()
    await expect(store.loadRecords({ allowEmpty: true })).rejects.toMatchObject({
      code: 'FLOW_STORAGE_MIGRATION_REQUIRED',
    })
    await expect(store.commit(commitFor(1, 'asset-1'))).rejects.toMatchObject({
      code: 'FLOW_STORAGE_MIGRATION_REQUIRED',
    })

    const upgraded = await openCurrentDatabase()
    expect(Array.from(upgraded.objectStoreNames)).toEqual(
      expect.arrayContaining([
        'snapshots',
        'snapshots-v3',
        'transactions-v3',
        'audits-v3',
        'assets-v3',
        'migration-sources-v3',
        'storage-metadata',
        'document-heads',
      ]),
    )
    for (const [storeName, expected] of Object.entries(legacyRecords)) {
      const transaction = upgraded.transaction(storeName, 'readonly')
      const records = await requestResultForTest<unknown[]>(
        transaction.objectStore(storeName).getAll(),
      )
      expect(records).toEqual(expected)
    }
    const currentStoreNames = ['snapshots-v3', 'transactions-v3', 'audits-v3', 'assets-v3']
    const currentTransaction = upgraded.transaction(currentStoreNames, 'readonly')
    await expect(
      Promise.all(
        currentStoreNames.map((storeName) =>
          requestResultForTest<unknown[]>(currentTransaction.objectStore(storeName).getAll()),
        ),
      ),
    ).resolves.toEqual([[], [], [], []])
    upgraded.close()
  })
})

function openLegacyDatabase(
  records: Readonly<Record<'snapshots' | 'transactions' | 'audits', readonly unknown[]>>,
): Promise<IDBDatabase> {
  return new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(databaseName, 2)
    request.onupgradeneeded = () => {
      for (const [storeName, stored] of Object.entries(records)) {
        const store = request.result.createObjectStore(storeName, { autoIncrement: true })
        for (const record of stored) store.put(record)
      }
    }
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

function openCurrentDatabase(): Promise<IDBDatabase> {
  return new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(databaseName)
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

function openV4Database(commit: PersistenceCommitDto): Promise<IDBDatabase> {
  return new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(databaseName, 4)
    request.onupgradeneeded = () => {
      const stores = {
        'snapshots-v3': [commit.snapshot],
        'transactions-v3': [commit.transaction],
        'audits-v3': [commit.audit],
        'assets-v3': commit.assets,
        'migration-sources-v3': [],
      }
      for (const [storeName, records] of Object.entries(stores)) {
        const store = request.result.createObjectStore(storeName, { keyPath: 'physicalKey' })
        records.forEach((record, index) => {
          store.put({ physicalKey: `${storeName}-${index}`, record })
        })
      }
      request.result.createObjectStore('storage-metadata', { keyPath: 'key' })
    }
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

function deleteDatabaseForTest(name: string): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    const request = indexedDB.deleteDatabase(name)
    request.onsuccess = () => resolve()
    request.onerror = () => reject(request.error)
  })
}

interface RawStoredRecord {
  readonly storeName: string
  readonly physicalKey: string
  readonly record: unknown
}

async function injectRawRecords(records: readonly RawStoredRecord[]): Promise<void> {
  const database = await openCurrentDatabase()
  const storeNames = [...new Set(records.map(({ storeName }) => storeName))]
  const transaction = database.transaction(storeNames, 'readwrite', {
    durability: 'strict',
  })
  for (const { storeName, physicalKey, record } of records) {
    transaction.objectStore(storeName).put({ physicalKey, record })
  }
  await transactionCompleteForTest(transaction)
  database.close()
}

function requestResultForTest<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

function transactionCompleteForTest(transaction: IDBTransaction): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    transaction.oncomplete = () => resolve()
    transaction.onabort = () => reject(transaction.error)
    transaction.onerror = () => {
      // The abort is the terminal event.
    }
  })
}
