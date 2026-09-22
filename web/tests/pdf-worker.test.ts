import { describe, expect, it } from 'vitest'

import {
  PDF_PROTOCOL_VERSION,
  type PdfExportRequestDto,
  type PdfExportWorkerResultDto,
  type PdfRecoveryResultDto,
  validatePdfExportRequest,
  validatePdfExportResult,
} from '../src/pdf/pdf-protocol.js'
import {
  PdfWorkerError,
  RevisionAwarePdfExportScheduler,
  createWasmPdfExportEngine,
  createWasmPdfExportScheduler,
  createWorkerPdfExportScheduler,
  installPdfWorker,
  recoverWithWasm,
  type PdfExportEngine,
  type PdfWorkerPort,
  type PdfWorkerScope,
} from '../src/pdf/pdf-worker.js'
import type { PdfWorkerOutboundMessage } from '../src/pdf/pdf-protocol.js'

function request(revision: number, id = `pdf-request-${revision}`): PdfExportRequestDto {
  return {
    protocolVersion: PDF_PROTOCOL_VERSION,
    requestId: id,
    sourceRevision: revision,
    sourceHash: `source-${revision}`,
    layoutSettingsFingerprint: 'settings-v1',
    layoutResultHash: `layout-${revision}`,
    serializedRequest: '{}',
  }
}

function result(
  sourceRequest: PdfExportRequestDto,
  overrides: Partial<PdfExportWorkerResultDto> = {},
): PdfExportWorkerResultDto {
  return {
    protocolVersion: PDF_PROTOCOL_VERSION,
    requestId: sourceRequest.requestId,
    sourceRevision: sourceRequest.sourceRevision,
    sourceHash: sourceRequest.sourceHash,
    layoutSettingsFingerprint: sourceRequest.layoutSettingsFingerprint,
    exportFingerprint: `export-${sourceRequest.sourceRevision}`,
    byteHash: `byte-${sourceRequest.sourceRevision}`,
    bytesHex: '25504446',
    privateSourceStreamHex: '464c4f57',
    manifest: {
      manifestVersion: 1,
      exportSchemaVersion: 1,
      sourceDocumentId: '00000000-0000-4000-8000-000000000001',
      sourceRevision: sourceRequest.sourceRevision,
      sourceSchemaVersion: 2,
      sourceHash: sourceRequest.sourceHash,
      sourcePayloadHash: 'payload-hash',
      layoutSettingsFingerprint: sourceRequest.layoutSettingsFingerprint,
      layoutResultHash: sourceRequest.layoutResultHash,
      engine: 'flow-core',
      engineVersion: '0.1.0',
      fontCatalogIdentity: null,
      fontFaces: [],
      hyphenationIdentity: null,
      options: {},
      exportFingerprint: `export-${sourceRequest.sourceRevision}`,
    },
    supportReport: { supported: ['text'], unsupported: [] },
    ...overrides,
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

describe('revision-aware PDF worker scheduling', () => {
  it('supersedes an older export without replacing the newest accepted result', async () => {
    const pending: Array<ReturnType<typeof deferred<PdfExportWorkerResultDto>>> = []
    const engine: PdfExportEngine = async () => {
      const next = deferred<PdfExportWorkerResultDto>()
      pending.push(next)
      return next.promise
    }
    const scheduler = new RevisionAwarePdfExportScheduler(engine, {
      verifyResultHash: (value) => value.byteHash.startsWith('byte-'),
    })

    const firstRequest = request(1)
    const secondRequest = request(2)
    const first = scheduler.request(firstRequest)
    const second = scheduler.request(secondRequest)
    pending[1]?.resolve(result(secondRequest))
    expect(await second).toMatchObject({
      kind: 'published',
      accepted: { request: { requestId: secondRequest.requestId } },
    })
    pending[0]?.resolve(result(firstRequest))
    expect(await first).toMatchObject({ kind: 'cancelled', requestId: firstRequest.requestId })
    expect(scheduler.accepted()?.result.sourceRevision).toBe(2)
  })

  it('rejects source, layout, manifest, and byte identity mismatches', async () => {
    const cases: Array<{
      readonly override: Partial<PdfExportWorkerResultDto>
      readonly code: string
    }> = [
      { override: { requestId: 'other' }, code: 'FLOW_PDF_STALE_REQUEST_ID' },
      { override: { sourceRevision: 9 }, code: 'FLOW_PDF_STALE_SOURCE_REVISION' },
      { override: { sourceHash: 'other' }, code: 'FLOW_PDF_STALE_SOURCE_HASH' },
      {
        override: { layoutSettingsFingerprint: 'settings-v2' },
        code: 'FLOW_PDF_STALE_LAYOUT_SETTINGS',
      },
      {
        override: {
          manifest: { ...result(request(1)).manifest, sourceRevision: 9 },
        },
        code: 'FLOW_PDF_MANIFEST_SOURCE_REVISION_MISMATCH',
      },
      {
        override: {
          manifest: { ...result(request(1)).manifest, sourceHash: 'other' },
        },
        code: 'FLOW_PDF_MANIFEST_SOURCE_HASH_MISMATCH',
      },
      {
        override: {
          manifest: { ...result(request(1)).manifest, layoutSettingsFingerprint: 'settings-v2' },
        },
        code: 'FLOW_PDF_MANIFEST_LAYOUT_SETTINGS_MISMATCH',
      },
      {
        override: {
          manifest: { ...result(request(1)).manifest, exportFingerprint: 'other' },
        },
        code: 'FLOW_PDF_MANIFEST_EXPORT_FINGERPRINT_MISMATCH',
      },
      {
        override: { manifest: { ...result(request(1)).manifest, layoutResultHash: 'other' } },
        code: 'FLOW_PDF_STALE_LAYOUT_RESULT',
      },
      { override: { bytesHex: 'not-hex' }, code: 'FLOW_PDF_BYTES_INVALID' },
    ]

    for (const [index, testCase] of cases.entries()) {
      const sourceRequest = request(1, `guard-${index}`)
      const scheduler = new RevisionAwarePdfExportScheduler(
        async (requestValue) => result(requestValue, testCase.override),
        { verifyResultHash: () => true },
      )
      const outcome = await scheduler.request(sourceRequest)
      expect(outcome).toMatchObject({ kind: 'discarded', code: testCase.code })
      expect(scheduler.accepted()).toBeNull()
    }

    const invalidRequest = {
      ...request(1),
      layoutResultHash: '',
    }
    expect(validatePdfExportRequest(invalidRequest)).toBe('FLOW_PDF_LAYOUT_RESULT_INVALID')
    expect(validatePdfExportResult(request(1), result(request(1), { privateSourceStreamHex: '' })))
      .toBe('FLOW_PDF_SOURCE_STREAM_INVALID')
  })

  it('cancels cooperatively and reports engine failures without authored data', async () => {
    const next = deferred<PdfExportWorkerResultDto>()
    let calls = 0
    const engine: PdfExportEngine = async (_request, signal) => {
      calls += 1
      expect(signal).toBeInstanceOf(AbortSignal)
      if (calls === 1) return next.promise
      throw new PdfWorkerError('FLOW_PDF_ENGINE_FAILED')
    }
    const diagnostics: string[] = []
    const scheduler = new RevisionAwarePdfExportScheduler(engine, {
      verifyResultHash: () => true,
      onDiagnostic: ({ code }) => diagnostics.push(code),
    })

    const cancelled = scheduler.request(request(1, 'cancel-me'))
    scheduler.cancel('cancel-me')
    next.resolve(result(request(1, 'cancel-me')))
    expect(await cancelled).toMatchObject({ kind: 'cancelled' })

    const failed = await scheduler.request(request(2, 'fail-once'))
    expect(failed).toMatchObject({ kind: 'failed', code: 'FLOW_PDF_ENGINE_FAILED' })
    expect(diagnostics).toEqual(['FLOW_PDF_ENGINE_FAILED'])
  })

  it('rejects malformed or oversized string-only requests', () => {
    expect(
      validatePdfExportRequest({
        ...request(1),
        serializedRequest: 'x'.repeat(96 * 1024 * 1024 + 1),
      }),
    ).toBe('FLOW_PDF_REQUEST_LIMIT')
  })

  it('composes the WASM adapter with the revision-aware scheduler', async () => {
    const sourceRequest = request(1, 'wasm-scheduler-request')
    const expected = result(sourceRequest)
    const exportResponse = JSON.stringify({
      protocolVersion: PDF_PROTOCOL_VERSION,
      requestId: sourceRequest.requestId,
      ok: true,
      result: expected,
      error: null,
    })
    let responseVerified = 0
    let published = 0
    const scheduler = createWasmPdfExportScheduler(
      {
        export_pdf: () => exportResponse,
        verify_pdf_export_response: () => {
          responseVerified += 1
          return true
        },
        recover_owned_source: () => '',
      },
      { onPublished: () => published += 1 },
    )

    const outcome = await scheduler.request(sourceRequest)
    expect(outcome).toMatchObject({
      kind: 'published',
      accepted: { request: { requestId: sourceRequest.requestId } },
    })
    expect(responseVerified).toBe(1)
    expect(published).toBe(1)
    scheduler.dispose()
  })

  it('routes accepted and cancelled results through the worker scope', async () => {
    const acceptedMessages: PdfWorkerOutboundMessage[] = []
    const acceptedScope: PdfWorkerScope = {
      onmessage: null,
      postMessage: (message) => acceptedMessages.push(message),
    }
    const acceptedWorker = installPdfWorker(
      acceptedScope,
      async (sourceRequest) => result(sourceRequest),
      () => true,
    )
    const acceptedRequest = request(1, 'worker-accepted')
    if (acceptedScope.onmessage === null) throw new Error('worker handler is required')
    acceptedScope.onmessage({ data: { type: 'export', request: acceptedRequest } })
    await tick()
    expect(acceptedMessages).toHaveLength(1)
    expect(acceptedMessages[0]).toMatchObject({
      type: 'accepted',
      requestId: acceptedRequest.requestId,
    })
    acceptedWorker.dispose()

    const pending = deferred<PdfExportWorkerResultDto>()
    const cancelledMessages: PdfWorkerOutboundMessage[] = []
    const cancelledScope: PdfWorkerScope = {
      onmessage: null,
      postMessage: (message) => cancelledMessages.push(message),
    }
    const cancelledWorker = installPdfWorker(
      cancelledScope,
      async () => pending.promise,
      () => true,
    )
    const cancelledRequest = request(1, 'worker-cancelled')
    if (cancelledScope.onmessage === null) throw new Error('worker handler is required')
    cancelledScope.onmessage({ data: { type: 'export', request: cancelledRequest } })
    cancelledScope.onmessage({
      data: { type: 'cancel', requestId: cancelledRequest.requestId },
    })
    pending.resolve(result(cancelledRequest))
    await tick()
    expect(cancelledMessages).toEqual([
      { type: 'cancelled', requestId: cancelledRequest.requestId },
    ])
    expect(cancelledWorker.accepted()).toBeNull()
    cancelledWorker.dispose()
  })

  it('adapts export responses and classifies exact recovery failures', async () => {
    const sourceRequest = request(1, 'wasm-request')
    const expected = result(sourceRequest)
    const exportResponse = JSON.stringify({
      protocolVersion: PDF_PROTOCOL_VERSION,
      requestId: sourceRequest.requestId,
      ok: true,
      result: expected,
      error: null,
    })
    const adapter = createWasmPdfExportEngine({
      export_pdf: () => exportResponse,
      verify_pdf_export_response: () => true,
      recover_owned_source: () => '',
    })
    const adapted = await adapter.run(sourceRequest, new AbortController().signal)
    expect(adapted.requestId).toBe(sourceRequest.requestId)
    expect(adapter.verifyResultHash(adapted)).toBe(true)

    const malformed = createWasmPdfExportEngine({
      export_pdf: () => '{not-json',
      verify_pdf_export_response: () => true,
      recover_owned_source: () => '',
    })
    await expect(malformed.run(sourceRequest, new AbortController().signal)).rejects.toMatchObject({
      code: 'FLOW_PDF_RESPONSE_DECODE',
    })

    const recovered: PdfRecoveryResultDto = {
      exact: true,
      canonicalJson: '{"documentId":"doc"}',
      canonicalHash: 'canonical-hash',
      documentId: '00000000-0000-4000-8000-000000000001',
      revision: 1,
      manifest: expected.manifest,
    }
    const recoveryWasm = {
      export_pdf: () => exportResponse,
      verify_pdf_export_response: () => true,
      recover_owned_source: () =>
        JSON.stringify({
          protocolVersion: PDF_PROTOCOL_VERSION,
          requestId: 'recovery',
          ok: true,
          result: recovered,
          error: null,
        }),
    }
    expect(
      recoverWithWasm(recoveryWasm, {
        protocolVersion: PDF_PROTOCOL_VERSION,
        requestId: 'recovery',
        privateSourceStreamHex: '464c4f57',
        expected: {
          documentId: null,
          revision: null,
          canonicalHash: null,
          exportFingerprint: null,
        },
      }),
    ).toEqual(recovered)

    const failedRecovery = {
      ...recoveryWasm,
      recover_owned_source: () =>
        JSON.stringify({
          protocolVersion: PDF_PROTOCOL_VERSION,
          requestId: 'recovery',
          ok: false,
          result: null,
          error: { code: 'FLOW_PDF_RECOVERY_PAYLOAD_MISSING' },
        }),
    }
    expect(() =>
      recoverWithWasm(failedRecovery, {
        protocolVersion: PDF_PROTOCOL_VERSION,
        requestId: 'recovery',
        privateSourceStreamHex: null,
        expected: {
          documentId: null,
          revision: null,
          canonicalHash: null,
          exportFingerprint: null,
        },
      }),
    ).toThrowError('FLOW_PDF_RECOVERY_PAYLOAD_MISSING')
  })

  it('delegates a worker-backed request through the existing scheduler guards', async () => {
    const posted: string[] = []
    const worker: PdfWorkerPort = {
      onmessage: null,
      onerror: null,
      onmessageerror: null,
      postMessage: (message) => {
        if (message.type !== 'export') return
        posted.push(message.request.requestId)
        queueMicrotask(() => {
          worker.onmessage?.({
            data: { type: 'accepted', requestId: message.request.requestId, result: result(message.request) },
          })
        })
      },
      terminate: () => undefined,
    }
    const scheduler = createWorkerPdfExportScheduler(worker)
    const outcome = await scheduler.request(request(3, 'worker-backed'))
    expect(outcome).toMatchObject({ kind: 'published' })
    expect(posted).toEqual(['worker-backed'])
    scheduler.dispose()
  })
})

async function tick(): Promise<void> {
  await new Promise<void>((resolve) => setTimeout(resolve, 0))
}
