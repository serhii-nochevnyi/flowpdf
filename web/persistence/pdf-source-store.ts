const DEFAULT_DATABASE_NAME = 'flowpdf-pdf-sources'
const DATABASE_VERSION = 1
const STORE_NAME = 'pdf-sources-v1'
const RECORD_VERSION = 1 as const
export const MAX_PDF_SOURCE_BYTES = 64 * 1024 * 1024
const MAX_SOURCE_HASH_BYTES = 128

export interface PdfSourceMetadataDto {
  readonly recordVersion: typeof RECORD_VERSION
  readonly sourceHash: string
  readonly byteLength: number
}

export interface PdfSourceRecordDto extends PdfSourceMetadataDto {
  readonly bytes: Uint8Array
}

interface StoredPdfSourceRecord extends PdfSourceMetadataDto {
  readonly bytes: Uint8Array
}

export class PdfSourceStoreError extends Error {
  constructor(readonly code: string) {
    super(code)
    this.name = 'PdfSourceStoreError'
  }
}

/**
 * Immutable, source-only PDF storage. The store has no update or delete API;
 * an identical retry is a no-op and a conflicting payload fails closed.
 */
export class PdfSourceStore {
  private databasePromise: Promise<IDBDatabase> | undefined

  constructor(private readonly databaseName = DEFAULT_DATABASE_NAME) {}

  async put(sourceHash: string, bytes: Uint8Array): Promise<PdfSourceMetadataDto> {
    assertSourceHash(sourceHash)
    const payload = cloneBytes(bytes)
    assertByteLimit(payload)
    const candidate: StoredPdfSourceRecord = {
      recordVersion: RECORD_VERSION,
      sourceHash,
      byteLength: payload.byteLength,
      bytes: payload,
    }
    const database = await this.open()
    return new Promise<PdfSourceMetadataDto>((resolve, reject) => {
      const transaction = database.transaction(STORE_NAME, 'readwrite')
      let failure: PdfSourceStoreError | undefined
      transaction.oncomplete = () => resolve(metadata(candidate))
      transaction.onabort = () =>
        reject(failure ?? sourceError('FLOW_PDF_SOURCE_STORE_ABORTED', transaction.error))
      transaction.onerror = () => {
        failure ??= sourceError('FLOW_PDF_SOURCE_STORE_WRITE_FAILED', transaction.error)
      }
      const request = transaction.objectStore(STORE_NAME).get(sourceHash)
      request.onsuccess = () => {
        try {
          const existing = request.result as StoredPdfSourceRecord | undefined
          if (existing !== undefined) {
            assertStoredRecord(existing)
            if (!samePayload(existing, candidate)) {
              throw sourceError('FLOW_PDF_SOURCE_IDENTITY_CONFLICT')
            }
            return
          }
          transaction.objectStore(STORE_NAME).add(candidate)
        } catch (error: unknown) {
          failure ??= asSourceError(error, 'FLOW_PDF_SOURCE_STORE_WRITE_FAILED')
          try {
            transaction.abort()
          } catch {
            // The transaction may already be aborting after a request error.
          }
        }
      }
      request.onerror = () => {
        failure ??= sourceError('FLOW_PDF_SOURCE_STORE_READ_FAILED', request.error)
        try {
          transaction.abort()
        } catch {
          // The terminal abort callback carries the recorded error.
        }
      }
    })
  }

  async get(sourceHash: string): Promise<Uint8Array | null> {
    assertSourceHash(sourceHash)
    const database = await this.open()
    const transaction = database.transaction(STORE_NAME, 'readonly')
    const request = transaction.objectStore(STORE_NAME).get(sourceHash)
    const record = await requestResult<StoredPdfSourceRecord | undefined>(request)
    await transactionComplete(transaction)
    if (record === undefined) return null
    assertStoredRecord(record)
    return cloneBytes(record.bytes)
  }

  async metadata(sourceHash: string): Promise<PdfSourceMetadataDto | null> {
    assertSourceHash(sourceHash)
    const database = await this.open()
    const transaction = database.transaction(STORE_NAME, 'readonly')
    const request = transaction.objectStore(STORE_NAME).get(sourceHash)
    const record = await requestResult<StoredPdfSourceRecord | undefined>(request)
    await transactionComplete(transaction)
    if (record === undefined) return null
    assertStoredRecord(record)
    return metadata(record)
  }

  close(): void {
    this.databasePromise?.then((database) => database.close()).catch(() => undefined)
    this.databasePromise = undefined
  }

  private open(): Promise<IDBDatabase> {
    if (this.databasePromise !== undefined) return this.databasePromise
    this.databasePromise = new Promise<IDBDatabase>((resolve, reject) => {
      let request: IDBOpenDBRequest
      try {
        request = indexedDB.open(this.databaseName, DATABASE_VERSION)
      } catch (error: unknown) {
        reject(asSourceError(error, 'FLOW_PDF_SOURCE_STORE_OPEN_FAILED'))
        return
      }
      request.onupgradeneeded = () => {
        const database = request.result
        if (!database.objectStoreNames.contains(STORE_NAME)) {
          database.createObjectStore(STORE_NAME, { keyPath: 'sourceHash' })
        }
      }
      request.onerror = () => reject(sourceError('FLOW_PDF_SOURCE_STORE_OPEN_FAILED', request.error))
      request.onblocked = () => reject(sourceError('FLOW_PDF_SOURCE_STORE_BLOCKED'))
      request.onsuccess = () => {
        const database = request.result
        database.onversionchange = () => {
          database.close()
          this.databasePromise = undefined
        }
        resolve(database)
      }
    })
    return this.databasePromise
  }
}

/** Browser-owned source identity used before the Rust reader returns its PDF hash. */
export async function hashPdfSourceBytes(bytes: Uint8Array): Promise<string> {
  assertByteLimit(bytes)
  try {
    const digestInput = new ArrayBuffer(bytes.byteLength)
    new Uint8Array(digestInput).set(bytes)
    const digest = await globalThis.crypto.subtle.digest('SHA-256', digestInput)
    return `sha256:v1:${bytesToHex(new Uint8Array(digest))}`
  } catch (error: unknown) {
    throw asSourceError(error, 'FLOW_PDF_SOURCE_HASH_FAILED')
  }
}

function assertSourceHash(sourceHash: string): void {
  if (
    sourceHash.length === 0 ||
    sourceHash.length > MAX_SOURCE_HASH_BYTES ||
    !/^(?:[0-9a-f]{64}|(?:sha256|pdf:blake3):v1:[0-9a-f]{64})$/.test(sourceHash)
  ) {
    throw sourceError('FLOW_PDF_SOURCE_HASH_INVALID')
  }
}

function assertByteLimit(bytes: Uint8Array): void {
  if (bytes.byteLength > MAX_PDF_SOURCE_BYTES) {
    throw sourceError('FLOW_PDF_SOURCE_SIZE_LIMIT')
  }
}

function assertStoredRecord(record: StoredPdfSourceRecord): void {
  if (
    record.recordVersion !== RECORD_VERSION ||
    typeof record.sourceHash !== 'string' ||
    record.byteLength !== record.bytes.byteLength
  ) {
    throw sourceError('FLOW_PDF_SOURCE_RECORD_INVALID')
  }
  assertSourceHash(record.sourceHash)
  assertByteLimit(record.bytes)
}

function samePayload(left: StoredPdfSourceRecord, right: StoredPdfSourceRecord): boolean {
  if (left.sourceHash !== right.sourceHash || left.byteLength !== right.byteLength) return false
  if (left.bytes.byteLength !== right.bytes.byteLength) return false
  for (let index = 0; index < left.bytes.byteLength; index += 1) {
    if (left.bytes[index] !== right.bytes[index]) return false
  }
  return true
}

function metadata(record: StoredPdfSourceRecord): PdfSourceMetadataDto {
  return {
    recordVersion: record.recordVersion,
    sourceHash: record.sourceHash,
    byteLength: record.byteLength,
  }
}

function cloneBytes(bytes: Uint8Array): Uint8Array {
  return new Uint8Array(bytes)
}

function bytesToHex(bytes: Uint8Array): string {
  let result = ''
  for (const byte of bytes) result += byte.toString(16).padStart(2, '0')
  return result
}

function requestResult<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    request.onsuccess = () => resolve(request.result)
    request.onerror = () => reject(sourceError('FLOW_PDF_SOURCE_STORE_READ_FAILED', request.error))
  })
}

function transactionComplete(transaction: IDBTransaction): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    transaction.oncomplete = () => resolve()
    transaction.onabort = () => reject(sourceError('FLOW_PDF_SOURCE_STORE_ABORTED', transaction.error))
    transaction.onerror = () => reject(sourceError('FLOW_PDF_SOURCE_STORE_READ_FAILED', transaction.error))
  })
}

function sourceError(code: string, cause?: unknown): PdfSourceStoreError {
  const error = new PdfSourceStoreError(code)
  if (cause !== undefined) error.cause = cause
  return error
}

function asSourceError(error: unknown, code: string): PdfSourceStoreError {
  return error instanceof PdfSourceStoreError ? error : sourceError(code, error)
}
