import { expect, test } from 'vitest'

import {
  mountFoundationInspector,
  type FoundationInspectorController,
  type FoundationInspectorSnapshot,
} from '../src/foundation-inspector'

interface MountOptionsContract {
  readonly databaseName: string
}

const mountWithOptions = mountFoundationInspector as unknown as (
  root: HTMLElement,
  options: MountOptionsContract,
) => Promise<FoundationInspectorController>

const databaseVersion = 3
const snapshotStore = `snapshots-v${databaseVersion}`
const transactionStore = `transactions-v${databaseVersion}`
const auditStore = `audits-v${databaseVersion}`

interface StoredEnvelope {
  readonly physicalKey: string
  readonly record: Record<string, unknown>
}

function action(root: HTMLElement, name: string): HTMLButtonElement {
  const control = root.querySelector<HTMLButtonElement>(`[data-action="${name}"]`)
  if (control === null) throw new Error(`missing ${name} control`)
  return control
}

async function clickAndWait(
  inspector: FoundationInspectorController,
  root: HTMLElement,
  name: string,
): Promise<void> {
  action(root, name).click()
  await inspector.whenIdle()
}

async function createDurableBaseline(
  databaseName: string,
): Promise<{
  readonly root: HTMLElement
  readonly inspector: FoundationInspectorController
  readonly snapshot: FoundationInspectorSnapshot
}> {
  const root = document.createElement('div')
  document.body.replaceChildren(root)
  const inspector = await mountWithOptions(root, { databaseName })
  await clickAndWait(inspector, root, 'create-sample')
  await clickAndWait(inspector, root, 'apply-mutation')
  await clickAndWait(inspector, root, 'save')
  return { root, inspector, snapshot: inspector.snapshot() }
}

test('recovery: an aborted incomplete physical transaction is invisible after page reconstruction', async () => {
  const databaseName = 'flowpdf-aborted-recovery-browser-test'
  const baseline = await createDurableBaseline(databaseName)
  const durableEnvelope = await firstEnvelope(databaseName, transactionStore)

  const database = await openDatabase(databaseName)
  const transaction = database.transaction(transactionStore, 'readwrite', {
    durability: 'strict',
  })
  transaction.objectStore(transactionStore).put({
    physicalKey: 'incomplete-record-that-must-rollback',
    record: {
      ...durableEnvelope.record,
      transactionId: '00000000-0000-4000-8000-000000009901',
      commandId: '00000000-0000-4000-8000-000000009901',
      baseRevision: 2,
      newRevision: 3,
    },
  })
  transaction.abort()
  await expectTransactionAbort(transaction)
  database.close()

  const reconstructedRoot = document.createElement('div')
  document.body.replaceChildren(reconstructedRoot)
  const reconstructed = await mountWithOptions(reconstructedRoot, { databaseName })
  await clickAndWait(reconstructed, reconstructedRoot, 'open-last')

  const reopened = reconstructed.snapshot()
  expect(reopened.revision).toBe(baseline.snapshot.revision)
  expect(reopened.hash).toBe(baseline.snapshot.hash)
  expect(reopened.audit).toHaveLength(baseline.snapshot.audit.length + 1)
  expect(reopened.audit.find(({ action }) => action.type === 'recovery')).toMatchObject({
    baseRevision: baseline.snapshot.revision,
    newRevision: baseline.snapshot.revision,
    action: { type: 'recovery' },
    outcome: { kind: 'success' },
  })
  expect(await countRecords(databaseName, transactionStore)).toBe(2)
})

for (const corruption of ['hash', 'gap', 'conflict'] as const) {
  test(`recovery: ${corruption} corruption fails closed and retains the prior verified display`, async () => {
    const databaseName = `flowpdf-${corruption}-recovery-browser-test`
    const { root, inspector, snapshot } = await createDurableBaseline(databaseName)
    await injectCorruption(databaseName, corruption)

    await clickAndWait(inspector, root, 'recover')

    expect(inspector.snapshot()).toEqual(snapshot)
    expect(root.querySelector('[role="alert"]')?.textContent).toMatch(
      /FLOW_(HASH_MISMATCH|RECOVERY_GAP)/,
    )
    const auditRecords = await allEnvelopes(databaseName, auditStore)
    const failure = auditRecords.find(
      ({ record }) =>
        (record.action as { readonly type?: string } | undefined)?.type === 'recovery' &&
        (record.outcome as { readonly kind?: string } | undefined)?.kind === 'failure',
    )
    expect(failure?.record).toMatchObject({
      documentId: snapshot.documentId,
      baseRevision: snapshot.revision,
      newRevision: snapshot.revision,
      action: { type: 'recovery' },
      outcome: { kind: 'failure' },
    })
    expect(JSON.stringify(failure?.record)).not.toMatch(
      /canonicalJson|typed mutation|Український|English|commandArguments/i,
    )
    expect(root.textContent).not.toMatch(/canonicalJson|typed mutation|Український|English/i)
  })
}

test('recovery: a cold remount derives and durably records corruption from the atomic head', async () => {
  const databaseName = 'flowpdf-cold-corrupt-recovery-browser-test'
  const baseline = await createDurableBaseline(databaseName)
  await injectCorruption(databaseName, 'hash')

  const coldRoot = document.createElement('div')
  document.body.replaceChildren(coldRoot)
  const coldInspector = await mountWithOptions(coldRoot, { databaseName })
  await clickAndWait(coldInspector, coldRoot, 'open-last')

  expect(coldRoot.querySelector('[role="alert"]')?.textContent).toMatch(
    /FLOW_(HASH_MISMATCH|RECOVERY_GAP)/,
  )
  const auditRecords = await allEnvelopes(databaseName, auditStore)
  const failure = auditRecords.find(
    ({ record }) =>
      (record.action as { readonly type?: string } | undefined)?.type === 'recovery' &&
      (record.outcome as { readonly kind?: string } | undefined)?.kind === 'failure',
  )
  expect(failure?.record).toMatchObject({
    documentId: baseline.snapshot.documentId,
    baseRevision: baseline.snapshot.revision,
    newRevision: baseline.snapshot.revision,
    action: { type: 'recovery' },
    outcome: { kind: 'failure' },
  })
})

async function injectCorruption(
  databaseName: string,
  kind: 'hash' | 'gap' | 'conflict',
): Promise<void> {
  const storeName = kind === 'hash' ? snapshotStore : transactionStore
  const original = await firstEnvelope(databaseName, storeName)
  let record: Record<string, unknown>
  if (kind === 'hash') {
    record = { ...original.record, canonicalHash: `blake3:${'0'.repeat(64)}` }
  } else if (kind === 'gap') {
    record = {
      ...original.record,
      transactionId: '00000000-0000-4000-8000-000000009902',
      commandId: '00000000-0000-4000-8000-000000009902',
      baseRevision: 40,
      newRevision: 41,
    }
  } else {
    record = { ...original.record, issuedAt: '2026-08-14T23:59:59Z' }
  }

  const database = await openDatabase(databaseName)
  const transaction = database.transaction(storeName, 'readwrite', { durability: 'strict' })
  transaction.objectStore(storeName).put({
    physicalKey: `injected-${kind}`,
    record,
  })
  await transactionTerminal(transaction)
  database.close()
}

async function firstEnvelope(databaseName: string, storeName: string): Promise<StoredEnvelope> {
  const records = await allEnvelopes(databaseName, storeName)
  const record = records[0]
  if (record === undefined) throw new Error(`missing durable record in ${storeName}`)
  return record
}

async function allEnvelopes(
  databaseName: string,
  storeName: string,
): Promise<StoredEnvelope[]> {
  const database = await openDatabase(databaseName)
  const transaction = database.transaction(storeName, 'readonly')
  const records = await requestResult<StoredEnvelope[]>(
    transaction.objectStore(storeName).getAll(),
  )
  await transactionTerminal(transaction)
  database.close()
  return records
}

async function countRecords(databaseName: string, storeName: string): Promise<number> {
  const database = await openDatabase(databaseName)
  const transaction = database.transaction(storeName, 'readonly')
  const count = await requestResult(transaction.objectStore(storeName).count())
  await transactionTerminal(transaction)
  database.close()
  return count
}

function openDatabase(databaseName: string): Promise<IDBDatabase> {
  return new Promise<IDBDatabase>((resolve, reject) => {
    const request = indexedDB.open(databaseName)
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

function requestResult<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(request.error)
  })
}

function transactionTerminal(transaction: IDBTransaction): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    transaction.oncomplete = () => resolve()
    transaction.onabort = () => reject(transaction.error ?? new Error('IndexedDB transaction aborted'))
    transaction.onerror = () => reject(transaction.error ?? new Error('IndexedDB transaction failed'))
  })
}

function expectTransactionAbort(transaction: IDBTransaction): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    transaction.oncomplete = () => reject(new Error('IndexedDB transaction unexpectedly completed'))
    transaction.onabort = () => resolve()
    transaction.onerror = () => {
      // The following abort event is the expected terminal signal.
    }
  })
}
