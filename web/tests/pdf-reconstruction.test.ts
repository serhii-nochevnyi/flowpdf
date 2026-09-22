import { describe, expect, it } from 'vitest'

import {
  PDF_RECONSTRUCTION_PROTOCOL_VERSION,
  type PdfDocumentCandidateDto,
  type PdfOcrAdapter,
  type PdfOcrResponseDto,
  type PdfReconstructionRequestDto,
  type PdfReconstructionResultDto,
  type PdfReconstructionWorkerResultDto,
  type PdfReviewDecisionDto,
  validatePdfReconstructionRequest,
  validatePdfReconstructionResult,
} from '../src/pdf/pdf-reconstruction-protocol.js'
import type { PdfReaderSceneDto } from '../src/pdf/pdf-protocol.js'
import {
  PdfReconstructionWorkerError,
  RevisionAwarePdfReconstructionScheduler,
  createWasmPdfReconstructionEngine,
  installPdfReconstructionWorker,
  type PdfReconstructionEngine,
  type PdfReconstructionEngineAdapter,
  type PdfReconstructionWorkerScope,
} from '../src/pdf/pdf-reconstruction-worker.js'
import { PdfOcrAdapterError } from '../src/pdf/pdf-reconstruction-protocol.js'

const SOURCE_HASH = 'a'.repeat(64)

function request(
  id = 'reconstruction-test-1',
  kind: 'text' | 'image' = 'text',
): PdfReconstructionRequestDto {
  return {
    protocolVersion: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
    requestId: id,
    sourceHash: SOURCE_HASH,
    scene: scene(kind),
    locale: 'uk-UA',
    ocrCandidates: [],
  }
}

function scene(kind: 'text' | 'image'): PdfReaderSceneDto {
  const report = { diagnostics: [], partial: false } as const
  return {
    schemaVersion: 1,
    sourceHash: SOURCE_HASH,
    pageCount: 1,
    firstPage: 0,
    pages: [
      {
        pageIndex: 0,
        sourceObject: { objectNumber: 3, generation: 0 },
        bounds: { x: 0, y: 0, width: 12_800, height: 12_800 },
        elements:
          kind === 'text'
            ? [
                {
                  kind: 'text' as const,
                  value: {
                    text: 'Вітаю',
                    rect: { x: 640, y: 10_880, width: 2_000, height: 640 },
                    fontName: null,
                    mapping: 'toUnicode' as const,
                    glyphs: [],
                    provenance: {
                      objectRef: { objectNumber: 4, generation: 0 },
                      streamOffset: 1,
                      operator: 'Tj',
                    },
                  },
                },
              ]
            : [
                {
                  kind: 'image' as const,
                  value: {
                    width: 8,
                    height: 8,
                    mediaType: 'image/png',
                    filter: 'FlateDecode',
                    dataHex: '89504e47',
                    rect: { x: 640, y: 9_600, width: 4_000, height: 2_000 },
                    provenance: {
                      objectRef: { objectNumber: 5, generation: 0 },
                      streamOffset: 2,
                      operator: 'Do',
                    },
                  },
                },
              ],
        report,
        partial: false,
      },
    ],
    report,
    resultHash: 'scene-result-hash',
  }
}

function result(
  sourceRequest: PdfReconstructionRequestDto,
  reviewRequired = false,
): PdfReconstructionWorkerResultDto {
  const document = {
    schemaVersion: 2,
    provenance: {
      kind: 'externalReconstruction',
      sourceHash: `pdf:blake3:v1:${sourceRequest.sourceHash}`,
      reconstructionSchemaVersion: 1,
    },
  }
  const candidate: PdfDocumentCandidateDto = {
    document,
    canonicalJson: JSON.stringify(document),
    canonicalHash: 'flowpdf:blake3:v1:candidate',
  }
  return {
    protocolVersion: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
    requestId: sourceRequest.requestId,
    schemaVersion: 1,
    sourceHash: sourceRequest.sourceHash,
    sceneResultHash: sourceRequest.scene.resultHash,
    candidate,
    blocks: [
      {
        nodeId: '00000000-0000-4000-8000-000000000001',
        text: reviewRequired ? 'OCR текст' : 'Вітаю',
        rect: { x: 640, y: 10_880, width: 2_000, height: 640 },
        confidenceBasisPoints: reviewRequired ? 4_000 : 10_000,
        reviewRequired,
        mappings: [],
      },
    ],
    opaqueIslands: [],
    report: {
      readerReport: { diagnostics: [], partial: false },
      diagnostics: [],
      reviewRequiredCount: reviewRequired ? 1 : 0,
      partial: false,
    },
    resultHash: 'reconstruction-result-hash',
  }
}

function acceptedCandidate(resultValue: PdfReconstructionResultDto): PdfDocumentCandidateDto {
  return resultValue.candidate
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

function scheduler(
  engine: PdfReconstructionEngine,
  ocrAdapter?: PdfOcrAdapter,
): RevisionAwarePdfReconstructionScheduler {
  const options = {
    verifyResultHash: () => true,
    verifyAcceptedCandidate: () => true,
    ...(ocrAdapter === undefined ? {} : { ocrAdapter }),
  }
  return new RevisionAwarePdfReconstructionScheduler(
    engine,
    async (acceptRequest) => ({ candidate: acceptedCandidate(acceptRequest.result) }),
    options,
  )
}

describe('revision-aware PDF reconstruction worker', () => {
  it('publishes text-only reconstruction and explicit review state', async () => {
    const engine: PdfReconstructionEngine = async (sourceRequest) =>
      result(sourceRequest, false)
    const reconstruction = scheduler(engine)
    const outcome = await reconstruction.request(request())
    expect(outcome).toMatchObject({ kind: 'published' })
    expect(reconstruction.snapshot()).toMatchObject({ phase: 'ready' })
    expect(reconstruction.accepted()?.result.report.reviewRequiredCount).toBe(0)

    const reviewedRequest = request('reconstruction-review-1', 'image')
    const reviewed = scheduler(async (sourceRequest) => result(sourceRequest, true))
    await expect(reviewed.request(reviewedRequest)).resolves.toMatchObject({ kind: 'published' })
    const decisions: PdfReviewDecisionDto[] = [
      {
        nodeId: '00000000-0000-4000-8000-000000000001',
        action: { kind: 'keep' },
      },
    ]
    await expect(reviewed.accept(decisions)).resolves.toMatchObject({ kind: 'accepted' })
  })

  it('calls OCR only for explicitly selected scanned pages and fails closed when unavailable', async () => {
    const seen: PdfOcrResponseDto[] = []
    const ocrAdapter: PdfOcrAdapter = {
      recognize: async (ocrRequest) => {
        expect(ocrRequest.pages.map((page) => page.pageIndex)).toEqual([0])
        const response = {
          sourceHash: ocrRequest.sourceHash,
          candidates: [
            {
              pageIndex: 0,
              rect: { x: 640, y: 9_600, width: 2_000, height: 640 },
              text: 'OCR текст',
              confidenceBasisPoints: 4_000,
              provider: 'fixture-ocr',
            },
          ],
        }
        seen.push(response)
        return response
      },
    }
    const enrichedRequests: PdfReconstructionRequestDto[] = []
    const engine: PdfReconstructionEngine = async (sourceRequest) => {
      enrichedRequests.push(sourceRequest)
      return result(sourceRequest, true)
    }
    const reconstruction = scheduler(engine, ocrAdapter)
    const sourceRequest = { ...request('reconstruction-ocr-1', 'image'), ocrPageIndexes: [0] }
    await expect(reconstruction.request(sourceRequest)).resolves.toMatchObject({ kind: 'published' })
    expect(seen).toHaveLength(1)
    expect(enrichedRequests[0]?.ocrCandidates).toHaveLength(1)
    expect(enrichedRequests[0]?.ocrCandidates[0]?.provider).toBe('fixture-ocr')

    const unavailable = scheduler(async (sourceRequest) => result(sourceRequest, true))
    await expect(unavailable.request(sourceRequest)).resolves.toMatchObject({
      kind: 'failed',
      code: 'FLOW_PDF_RECONSTRUCTION_OCR_UNAVAILABLE',
    })

    const providerFailure = scheduler(
      async (sourceRequest) => result(sourceRequest, true),
      {
        recognize: async () => {
          throw new PdfOcrAdapterError('FLOW_PDF_RECONSTRUCTION_OCR_PROVIDER_FAILED')
        },
      },
    )
    await expect(providerFailure.request(sourceRequest)).resolves.toMatchObject({
      kind: 'failed',
      code: 'FLOW_PDF_RECONSTRUCTION_OCR_PROVIDER_FAILED',
    })
  })

  it('cancels stale requests and rejects malformed identities', async () => {
    const pending: Array<ReturnType<typeof deferred<PdfReconstructionWorkerResultDto>>> = []
    const engine: PdfReconstructionEngine = async () => {
      const next = deferred<PdfReconstructionWorkerResultDto>()
      pending.push(next)
      return next.promise
    }
    const reconstruction = scheduler(engine)
    const firstRequest = request('reconstruction-old')
    const secondRequest = request('reconstruction-new')
    const first = reconstruction.request(firstRequest)
    await Promise.resolve()
    await Promise.resolve()
    const second = reconstruction.request(secondRequest)
    await Promise.resolve()
    await Promise.resolve()
    pending[1]?.resolve(result(secondRequest))
    await expect(second).resolves.toMatchObject({ kind: 'published' })
    pending[0]?.resolve(result(firstRequest))
    await expect(first).resolves.toMatchObject({
      kind: 'cancelled',
      requestId: firstRequest.requestId,
    })
    expect(reconstruction.accepted()?.request.requestId).toBe(secondRequest.requestId)

    expect(validatePdfReconstructionRequest({ ...request(), sourceHash: 'bad' })).toBe(
      'FLOW_PDF_RECONSTRUCTION_SOURCE_INVALID',
    )
    expect(
      validatePdfReconstructionResult(request(), {
        ...result(request()),
        sceneResultHash: 'other-scene',
      }),
    ).toBe('FLOW_PDF_RECONSTRUCTION_SCENE_INVALID')
  })

  it('verifies the WASM response and strips scheduler-only OCR page selectors', async () => {
    const sourceRequest = request('reconstruction-wasm')
    const expected = result(sourceRequest)
    let serializedRequest: unknown = null
    let verified = 0
    const adapter: PdfReconstructionEngineAdapter = createWasmPdfReconstructionEngine({
      reconstruct_pdf: (serialized) => {
        serializedRequest = JSON.parse(serialized) as unknown
        return JSON.stringify({
          protocolVersion: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
          requestId: sourceRequest.requestId,
          ok: true,
          result: { result: expected },
          error: null,
        })
      },
      verify_pdf_reconstruction_response: () => {
        verified += 1
        return true
      },
      accept_pdf_reconstruction: () =>
        JSON.stringify({
          protocolVersion: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
          requestId: 'accept',
          ok: true,
          result: { candidate: expected.candidate },
          error: null,
        }),
      verify_pdf_reconstruction_accept_response: () => true,
    })
    const selected = { ...sourceRequest, ocrPageIndexes: [0] }
    await expect(adapter.run(selected, new AbortController().signal)).resolves.toEqual(expected)
    expect(serializedRequest).not.toHaveProperty('ocrPageIndexes')
    expect(verified).toBe(1)
  })

  it('installs the isolated worker message loop', async () => {
    const messages: unknown[] = []
    const scope: PdfReconstructionWorkerScope = {
      onmessage: null,
      postMessage: (message) => messages.push(message),
    }
    const sourceRequest = request('reconstruction-worker')
    const adapter = createWasmPdfReconstructionEngine({
      reconstruct_pdf: () =>
        JSON.stringify({
          protocolVersion: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
          requestId: sourceRequest.requestId,
          ok: true,
          result: { result: result(sourceRequest) },
          error: null,
        }),
      verify_pdf_reconstruction_response: () => true,
      accept_pdf_reconstruction: () => '',
      verify_pdf_reconstruction_accept_response: () => false,
    })
    const worker = installPdfReconstructionWorker(scope, adapter)
    if (scope.onmessage === null) throw new Error('reconstruction worker handler is required')
    scope.onmessage({ data: { type: 'reconstruct', request: sourceRequest } })
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
    await new Promise<void>((resolve) => setTimeout(resolve, 0))
    expect(messages).toHaveLength(1)
    expect(messages[0]).toMatchObject({ type: 'accepted', requestId: sourceRequest.requestId })
    worker.dispose()
  })

  it('maps unknown engine errors to a stable worker code', async () => {
    const reconstruction = scheduler(async () => {
      throw new Error('not a protocol error')
    })
    await expect(reconstruction.request(request('reconstruction-error'))).resolves.toMatchObject({
      kind: 'failed',
      code: 'FLOW_PDF_RECONSTRUCTION_WORKER_FAILURE',
    })
    expect(new PdfReconstructionWorkerError('FLOW_TEST').code).toBe('FLOW_TEST')
  })
})
