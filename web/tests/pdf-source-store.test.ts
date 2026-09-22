import { IDBFactory } from 'fake-indexeddb'
import { afterEach, beforeEach, describe, expect, it } from 'vitest'

import {
  MAX_PDF_SOURCE_BYTES,
  PdfSourceStore,
  hashPdfSourceBytes,
} from '../persistence/pdf-source-store.js'

const databaseName = 'flowpdf-pdf-source-store-test'
const sourceHash = 'a'.repeat(64)

describe('PdfSourceStore', () => {
  let store: PdfSourceStore

  beforeEach(() => {
    globalThis.indexedDB = new IDBFactory() as unknown as IDBFactory
    store = new PdfSourceStore(databaseName)
  })

  afterEach(() => {
    store.close()
  })

  it('round-trips cloned bytes and exposes metadata without a mutation path', async () => {
    const bytes = new Uint8Array([0x25, 0x50, 0x44, 0x46])
    const metadata = await store.put(sourceHash, bytes)
    expect(metadata).toEqual({ recordVersion: 1, sourceHash, byteLength: 4 })
    bytes[0] = 0

    const loaded = await store.get(sourceHash)
    expect(Array.from(loaded ?? [])).toEqual([0x25, 0x50, 0x44, 0x46])
    if (loaded === null) throw new Error('source bytes are required')
    loaded[1] = 0
    await expect(store.get(sourceHash)).resolves.toEqual(new Uint8Array([0x25, 0x50, 0x44, 0x46]))
    await expect(store.metadata(sourceHash)).resolves.toEqual(metadata)
  })

  it('accepts identical retries and rejects conflicting bytes under the same identity', async () => {
    const bytes = new Uint8Array([1, 2, 3])
    await expect(store.put(sourceHash, bytes)).resolves.toMatchObject({ byteLength: 3 })
    await expect(store.put(sourceHash, new Uint8Array([1, 2, 3]))).resolves.toMatchObject({
      byteLength: 3,
    })
    await expect(store.put(sourceHash, new Uint8Array([1, 2, 4]))).rejects.toMatchObject({
      code: 'FLOW_PDF_SOURCE_IDENTITY_CONFLICT',
    })
  })

  it('rejects malformed identities and bounded payload violations without writing', async () => {
    await expect(store.put('not-a-source', new Uint8Array([1]))).rejects.toMatchObject({
      code: 'FLOW_PDF_SOURCE_HASH_INVALID',
    })
    await expect(
      store.put('b'.repeat(64), new Uint8Array(MAX_PDF_SOURCE_BYTES + 1)),
    ).rejects.toMatchObject({ code: 'FLOW_PDF_SOURCE_SIZE_LIMIT' })
    await expect(store.get('not-a-source')).rejects.toMatchObject({
      code: 'FLOW_PDF_SOURCE_HASH_INVALID',
    })
    await expect(store.get(sourceHash)).resolves.toBeNull()
  })

  it('derives a bounded browser-owned source identity before reader dispatch', async () => {
    const identity = await hashPdfSourceBytes(new Uint8Array([0x25, 0x50, 0x44, 0x46]))
    expect(identity).toMatch(/^sha256:v1:[0-9a-f]{64}$/)
  })
})
