import { IDBFactory, IDBObjectStore } from 'fake-indexeddb'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import {
  IndexedDbDocumentStore,
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

  it('keeps divergent opaque records visible for Rust conflict detection', async () => {
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
    await store.commit(divergent)

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

    await expect(store.loadRecords()).resolves.toEqual({
      snapshots: [migration.snapshot],
      transactions: [],
      audits: [migration.audit],
      assets: [migration.assets[0]],
      sources: [migration.source],
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

  it('adds versioned stores without deleting legacy stores or records', async () => {
    const legacyRecord = { legacy: true, value: 'must survive' }
    const legacyDatabase = await openLegacyDatabase(legacyRecord)
    legacyDatabase.close()

    const store = new IndexedDbDocumentStore()
    await store.loadRecords({ allowEmpty: true })

    const upgraded = await openCurrentDatabase()
    expect(Array.from(upgraded.objectStoreNames)).toEqual(
      expect.arrayContaining([
        'snapshots',
        'snapshots-v3',
        'transactions-v3',
        'audits-v3',
        'assets-v3',
        'migration-sources-v3',
      ]),
    )
    const transaction = upgraded.transaction('snapshots', 'readonly')
    const records = await requestResultForTest<unknown[]>(
      transaction.objectStore('snapshots').getAll(),
    )
    expect(records).toEqual([legacyRecord])
    upgraded.close()
  })
})

function openLegacyDatabase(record: unknown): Promise<IDBDatabase> {
  return new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(databaseName, 2)
    request.onupgradeneeded = () => {
      request.result.createObjectStore('snapshots', { autoIncrement: true }).put(record)
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

function requestResultForTest<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}
