import { describe, expect, it } from 'vitest'

import {
  PDF_READER_PROTOCOL_VERSION,
  type PdfReaderRequestDto,
  type PdfReaderWorkerResultDto,
  validatePdfReaderRequest,
  validatePdfReaderResult,
} from '../src/pdf/pdf-protocol.js'
import {
  PdfReaderWorkerError,
  RevisionAwarePdfReaderScheduler,
  createWasmPdfReaderEngine,
  installPdfReaderWorker,
  type PdfReaderEngine,
  type PdfReaderWorkerScope,
} from '../src/pdf/pdf-reader-worker.js'
import type { PdfReaderWorkerOutboundMessage } from '../src/pdf/pdf-protocol.js'

function request(id = 'reader-test-1'): PdfReaderRequestDto {
  return {
    protocolVersion: PDF_READER_PROTOCOL_VERSION,
    requestId: id,
    bytesHex: '255044462d312e37',
    firstPage: 0,
    pageCount: 1,
  }
}

function result(sourceRequest: PdfReaderRequestDto): PdfReaderWorkerResultDto {
  const report = { diagnostics: [], partial: false } as const
  return {
    protocolVersion: PDF_READER_PROTOCOL_VERSION,
    requestId: sourceRequest.requestId,
    summary: {
      schemaVersion: 1,
      pdfVersion: '1.7',
      sourceHash: 'source-hash',
      pageCount: 1,
    },
    scene: {
      schemaVersion: 1,
      sourceHash: 'source-hash',
      pageCount: 1,
      firstPage: 0,
      pages: [
        {
          pageIndex: 0,
          sourceObject: { objectNumber: 3, generation: 0 },
          bounds: { x: 0, y: 0, width: 12_800, height: 12_800 },
          elements: [],
          report,
          partial: false,
        },
      ],
      report,
      resultHash: 'scene-hash',
    },
    resultHash: 'scene-hash',
  }
}

function deferred<T>(): {
  readonly promise: Promise<T>
  readonly resolve: (value: T) => void
  readonly reject: (reason?: unknown) => void
} {
  let resolvePromise: (value: T) => void = () => undefined
  let rejectPromise: (reason?: unknown) => void = () => undefined
  const promise = new Promise<T>((resolve, reject) => {
    resolvePromise = resolve
    rejectPromise = reject
  })
  return { promise, resolve: resolvePromise, reject: rejectPromise }
}

describe('revision-aware PDF reader worker', () => {
  it('supersedes an older page-window request', async () => {
    const pending: Array<ReturnType<typeof deferred<PdfReaderWorkerResultDto>>> = []
    const engine: PdfReaderEngine = async () => {
      const next = deferred<PdfReaderWorkerResultDto>()
      pending.push(next)
      return next.promise
    }
    const scheduler = new RevisionAwarePdfReaderScheduler(engine, {
      verifyResultHash: () => true,
    })
    const firstRequest = request('reader-old')
    const secondRequest = request('reader-new')
    const first = scheduler.request(firstRequest)
    const second = scheduler.request(secondRequest)
    pending[1]?.resolve(result(secondRequest))
    expect(await second).toMatchObject({ kind: 'published' })
    pending[0]?.resolve(result(firstRequest))
    expect(await first).toMatchObject({ kind: 'cancelled', requestId: firstRequest.requestId })
    expect(scheduler.accepted()?.request.requestId).toBe(secondRequest.requestId)
  })

  it('validates bounds and protects diagnostics from authored content', async () => {
    expect(validatePdfReaderRequest({ ...request(), bytesHex: 'not-hex' })).toBe(
      'FLOW_PDF_READER_BYTES_INVALID',
    )
    expect(validatePdfReaderRequest({ ...request(), pageCount: 0 })).toBe(
      'FLOW_PDF_READER_PAGE_WINDOW_INVALID',
    )
    expect(validatePdfReaderResult(request(), result(request()))).toBeNull()
    expect(
      validatePdfReaderResult(request(), {
        ...result(request()),
        scene: { ...result(request()).scene, sourceHash: 'other' },
      }),
    ).toBe('FLOW_PDF_READER_SOURCE_HASH_MISMATCH')

    const diagnostics: string[] = []
    const scheduler = new RevisionAwarePdfReaderScheduler(
      async () => {
        throw new PdfReaderWorkerError('FLOW_PDF_READER_ENGINE_FAILED')
      },
      { verifyResultHash: () => true, onDiagnostic: ({ code }) => diagnostics.push(code) },
    )
    await expect(scheduler.request(request('reader-failure'))).resolves.toMatchObject({
      kind: 'failed',
      code: 'FLOW_PDF_READER_ENGINE_FAILED',
    })
    expect(diagnostics).toEqual(['FLOW_PDF_READER_ENGINE_FAILED'])
  })

  it('adapts verified WASM responses and installs a worker message loop', async () => {
    const sourceRequest = request('reader-wasm')
    const expected = result(sourceRequest)
    let verified = 0
    const adapter = createWasmPdfReaderEngine({
      read_pdf: () =>
        JSON.stringify({
          protocolVersion: PDF_READER_PROTOCOL_VERSION,
          requestId: sourceRequest.requestId,
          ok: true,
          result: expected,
          error: null,
        }),
      verify_pdf_reader_response: () => {
        verified += 1
        return true
      },
    })
    await expect(adapter.run(sourceRequest, new AbortController().signal)).resolves.toEqual(expected)
    expect(verified).toBe(1)

    const malformed = createWasmPdfReaderEngine({
      read_pdf: () =>
        JSON.stringify({
          protocolVersion: PDF_READER_PROTOCOL_VERSION,
          requestId: sourceRequest.requestId,
          ok: true,
          result: { ...expected, scene: null },
          error: null,
        }),
      verify_pdf_reader_response: () => true,
    })
    await expect(malformed.run(sourceRequest, new AbortController().signal)).rejects.toMatchObject({
      code: 'FLOW_PDF_READER_RESPONSE_DECODE',
    })

    const messages: PdfReaderWorkerOutboundMessage[] = []
    const scope: PdfReaderWorkerScope = { onmessage: null, postMessage: (message) => messages.push(message) }
    const worker = installPdfReaderWorker(scope, async (source) => result(source), () => true)
    if (scope.onmessage === null) throw new Error('reader worker handler is required')
    scope.onmessage({ data: { type: 'read', request: sourceRequest } })
    await Promise.resolve()
    await Promise.resolve()
    expect(messages).toEqual([{ type: 'accepted', requestId: sourceRequest.requestId, result: expected }])
    worker.dispose()
  })
})
