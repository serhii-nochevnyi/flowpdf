import {
  PDF_RECONSTRUCTION_PROTOCOL_VERSION,
  buildPdfOcrRequest,
  validatePdfOcrResponse,
  validatePdfReconstructionAcceptResult,
  validatePdfReconstructionRequest,
  validatePdfReconstructionResult,
  type AcceptedPdfReconstructionDto,
  type PdfDocumentCandidateDto,
  type PdfOcrAdapter,
  type PdfReconstructionAcceptRequestDto,
  type PdfReconstructionAcceptResultDto,
  type PdfReconstructionAcceptedCandidateDto,
  type PdfReconstructionRequestDto,
  type PdfReconstructionResultDto,
  type PdfReconstructionSchedulerSnapshotDto,
  type PdfReconstructionWasmBoundary,
  type PdfReconstructionWorkerInboundMessage,
  type PdfReconstructionWorkerOutboundMessage,
  type PdfReconstructionWorkerResultDto,
  type PdfReviewDecisionDto,
} from './pdf-reconstruction-protocol.js'

export interface PdfReconstructionEngine {
  (request: PdfReconstructionRequestDto, signal: AbortSignal): Promise<PdfReconstructionWorkerResultDto>
}

export interface PdfReconstructionAcceptanceEngine {
  (
    request: PdfReconstructionAcceptRequestDto,
    signal: AbortSignal,
  ): Promise<PdfReconstructionAcceptResultDto>
}

export interface PdfReconstructionEngineAdapter {
  readonly run: PdfReconstructionEngine
  readonly accept: PdfReconstructionAcceptanceEngine
  readonly verifyResultHash: (
    result: PdfReconstructionWorkerResultDto,
  ) => boolean | Promise<boolean>
  readonly verifyAcceptedCandidate: (
    candidate: PdfDocumentCandidateDto,
  ) => boolean | Promise<boolean>
}

export interface PdfReconstructionSchedulerOptions {
  readonly verifyResultHash: (
    result: PdfReconstructionWorkerResultDto,
  ) => boolean | Promise<boolean>
  readonly verifyAcceptedCandidate: (
    candidate: PdfDocumentCandidateDto,
  ) => boolean | Promise<boolean>
  readonly ocrAdapter?: PdfOcrAdapter
  readonly onPublished?: (accepted: AcceptedPdfReconstructionDto) => void
  readonly onAcceptedCandidate?: (accepted: PdfReconstructionAcceptedCandidateDto) => void
  readonly onDiagnostic?: (diagnostic: { readonly requestId: string; readonly code: string }) => void
}

export type PdfReconstructionScheduleOutcome =
  | { readonly kind: 'published'; readonly accepted: AcceptedPdfReconstructionDto }
  | { readonly kind: 'discarded'; readonly requestId: string; readonly code: string }
  | { readonly kind: 'cancelled'; readonly requestId: string }
  | { readonly kind: 'failed'; readonly requestId: string; readonly code: string }

export type PdfReconstructionAcceptOutcome =
  | {
      readonly kind: 'accepted'
      readonly accepted: PdfReconstructionAcceptedCandidateDto
    }
  | { readonly kind: 'cancelled'; readonly requestId: string }
  | { readonly kind: 'failed'; readonly requestId: string; readonly code: string }

export class PdfReconstructionWorkerError extends Error {
  constructor(readonly code: string) {
    super(code)
    this.name = 'PdfReconstructionWorkerError'
  }
}

interface ActiveRequest {
  readonly generation: number
  readonly requestId: string
  readonly controller: AbortController
}

/**
 * Single-flight reconstruction scheduler. OCR enrichment happens before the
 * Rust worker call but shares the same AbortSignal and generation, so a newer
 * source/scene cannot be overtaken by an older provider response.
 */
export class RevisionAwarePdfReconstructionScheduler {
  private readonly listeners = new Set<() => void>()
  private generation = 0
  private activeRequest: ActiveRequest | null = null
  private acceptedValue: AcceptedPdfReconstructionDto | null = null
  private acceptedCandidateValue: PdfReconstructionAcceptedCandidateDto | null = null
  private phase: PdfReconstructionSchedulerSnapshotDto['phase'] = 'idle'
  private pendingRequestId: string | null = null
  private errorValue: string | null = null
  private acceptSequence = 0

  constructor(
    private readonly engine: PdfReconstructionEngine,
    private readonly acceptanceEngine: PdfReconstructionAcceptanceEngine,
    private readonly options: PdfReconstructionSchedulerOptions,
  ) {}

  accepted(): AcceptedPdfReconstructionDto | null {
    return this.acceptedValue
  }

  acceptedCandidate(): PdfReconstructionAcceptedCandidateDto | null {
    return this.acceptedCandidateValue
  }

  snapshot(): PdfReconstructionSchedulerSnapshotDto {
    return {
      phase: this.phase,
      accepted: this.acceptedValue,
      acceptedCandidate: this.acceptedCandidateValue,
      requestId: this.pendingRequestId,
      errorCode: this.errorValue,
    }
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  request(request: PdfReconstructionRequestDto): Promise<PdfReconstructionScheduleOutcome> {
    const requestError = validatePdfReconstructionRequest(request)
    if (requestError !== null) {
      this.phase = 'error'
      this.pendingRequestId = request.requestId
      this.errorValue = requestError
      this.notify()
      this.report(request.requestId, requestError)
      return Promise.resolve({ kind: 'failed', requestId: request.requestId, code: requestError })
    }

    this.activeRequest?.controller.abort()
    const generation = this.generation + 1
    this.generation = generation
    const controller = new AbortController()
    this.activeRequest = { generation, requestId: request.requestId, controller }
    this.acceptedValue = null
    this.acceptedCandidateValue = null
    this.phase = 'pending'
    this.pendingRequestId = request.requestId
    this.errorValue = null
    this.notify()
    return this.run(request, generation, controller)
  }

  accept(
    decisions: readonly PdfReviewDecisionDto[],
    expected?: { readonly requestId: string; readonly sourceRequestId: string },
  ): Promise<PdfReconstructionAcceptOutcome> {
    const accepted = this.acceptedValue
    if (accepted === null) {
      return Promise.resolve({
        kind: 'failed',
        requestId: expected?.requestId ?? 'reconstruction-accept-unavailable',
        code: 'FLOW_PDF_RECONSTRUCTION_ACCEPT_UNAVAILABLE',
      })
    }
    if (
      this.activeRequest !== null ||
      (expected !== undefined && accepted.request.requestId !== expected.sourceRequestId)
    ) {
      const requestId = expected?.requestId ?? 'reconstruction-accept-pending'
      this.report(requestId, 'FLOW_PDF_RECONSTRUCTION_PENDING')
      return Promise.resolve({
        kind: 'failed',
        requestId,
        code: 'FLOW_PDF_RECONSTRUCTION_PENDING',
      })
    }

    const requestId = expected?.requestId ?? `reconstruction-accept-${++this.acceptSequence}`
    const sourceRequestId = expected?.sourceRequestId ?? accepted.request.requestId
    const request: PdfReconstructionAcceptRequestDto = {
      protocolVersion: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
      requestId,
      sourceRequestId,
      result: accepted.result,
      decisions,
    }
    const generation = this.generation + 1
    this.generation = generation
    const controller = new AbortController()
    this.activeRequest = { generation, requestId, controller }
    this.phase = 'pending'
    this.pendingRequestId = requestId
    this.errorValue = null
    this.notify()
    return this.runAccept(request, generation, controller)
  }

  cancel(requestId?: string): void {
    const active = this.activeRequest
    if (active === null || (requestId !== undefined && active.requestId !== requestId)) return
    active.controller.abort()
    this.generation += 1
    this.activeRequest = null
    this.phase = this.acceptedValue === null ? 'idle' : 'ready'
    this.pendingRequestId = null
    this.errorValue = null
    this.notify()
  }

  dispose(): void {
    this.cancel()
    this.listeners.clear()
  }

  private async run(
    request: PdfReconstructionRequestDto,
    generation: number,
    controller: AbortController,
  ): Promise<PdfReconstructionScheduleOutcome> {
    try {
      const enriched = await this.enrichWithOcr(request, controller.signal)
      if (controller.signal.aborted) {
        return { kind: 'cancelled', requestId: request.requestId }
      }
      const result = await this.engine(enriched, controller.signal)
      if (controller.signal.aborted) {
        return { kind: 'cancelled', requestId: request.requestId }
      }
      if (!this.isCurrent(generation, request.requestId)) {
        return {
          kind: 'discarded',
          requestId: request.requestId,
          code: 'FLOW_PDF_RECONSTRUCTION_STALE_REQUEST',
        }
      }
      const mismatch = validatePdfReconstructionResult(enriched, result)
      if (mismatch !== null) {
        this.finishWithError(mismatch)
        this.report(request.requestId, mismatch)
        return { kind: 'discarded', requestId: request.requestId, code: mismatch }
      }
      if (!(await this.options.verifyResultHash(result))) {
        this.finishWithError('FLOW_PDF_RECONSTRUCTION_RESULT_INVALID')
        this.report(request.requestId, 'FLOW_PDF_RECONSTRUCTION_RESULT_INVALID')
        return {
          kind: 'discarded',
          requestId: request.requestId,
          code: 'FLOW_PDF_RECONSTRUCTION_RESULT_INVALID',
        }
      }
      const accepted = Object.freeze({ request: enriched, result })
      this.acceptedValue = accepted
      this.acceptedCandidateValue = null
      this.phase = 'ready'
      this.pendingRequestId = null
      this.errorValue = null
      this.notify()
      this.options.onPublished?.(accepted)
      return { kind: 'published', accepted }
    } catch (error: unknown) {
      if (controller.signal.aborted) return { kind: 'cancelled', requestId: request.requestId }
      const code = errorCode(error)
      this.finishWithError(code)
      this.report(request.requestId, code)
      return { kind: 'failed', requestId: request.requestId, code }
    } finally {
      if (this.isCurrent(generation, request.requestId)) this.activeRequest = null
    }
  }

  private async runAccept(
    request: PdfReconstructionAcceptRequestDto,
    generation: number,
    controller: AbortController,
  ): Promise<PdfReconstructionAcceptOutcome> {
    try {
      const result = await this.acceptanceEngine(request, controller.signal)
      if (controller.signal.aborted) return { kind: 'cancelled', requestId: request.requestId }
      if (!this.isCurrent(generation, request.requestId)) {
        return {
          kind: 'failed',
          requestId: request.requestId,
          code: 'FLOW_PDF_RECONSTRUCTION_STALE_REQUEST',
        }
      }
      const mismatch = validatePdfReconstructionAcceptResult(request, result.candidate)
      if (mismatch !== null) {
        this.finishWithError(mismatch)
        this.report(request.requestId, mismatch)
        return { kind: 'failed', requestId: request.requestId, code: mismatch }
      }
      if (!(await this.options.verifyAcceptedCandidate(result.candidate))) {
        this.finishWithError('FLOW_PDF_RECONSTRUCTION_CANDIDATE_INVALID')
        this.report(request.requestId, 'FLOW_PDF_RECONSTRUCTION_CANDIDATE_INVALID')
        return {
          kind: 'failed',
          requestId: request.requestId,
          code: 'FLOW_PDF_RECONSTRUCTION_CANDIDATE_INVALID',
        }
      }
      const accepted = Object.freeze({ request, result })
      this.acceptedCandidateValue = accepted
      this.phase = 'ready'
      this.pendingRequestId = null
      this.errorValue = null
      this.notify()
      this.options.onAcceptedCandidate?.(accepted)
      return { kind: 'accepted', accepted }
    } catch (error: unknown) {
      if (controller.signal.aborted) return { kind: 'cancelled', requestId: request.requestId }
      const code = errorCode(error)
      this.finishWithError(code)
      this.report(request.requestId, code)
      return { kind: 'failed', requestId: request.requestId, code }
    } finally {
      if (this.isCurrent(generation, request.requestId)) this.activeRequest = null
    }
  }

  private async enrichWithOcr(
    request: PdfReconstructionRequestDto,
    signal: AbortSignal,
  ): Promise<PdfReconstructionRequestDto> {
    const pageIndexes = request.ocrPageIndexes
    if (pageIndexes === undefined || pageIndexes.length === 0) return request
    const pagesWithoutText = new Set(
      request.scene.pages
        .filter((page) => !page.elements.some((element) => element.kind === 'text'))
        .map((page) => page.pageIndex),
    )
    const requestedScannedPages = pageIndexes.filter((pageIndex) => pagesWithoutText.has(pageIndex))
    if (requestedScannedPages.length === 0) return request
    if (this.options.ocrAdapter === undefined) {
      throw new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_OCR_UNAVAILABLE')
    }
    const ocrRequest = buildPdfOcrRequest(request.scene, requestedScannedPages)
    if (ocrRequest === null) {
      throw new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_OCR_INPUT_LIMIT')
    }
    const response = await this.options.ocrAdapter.recognize(ocrRequest, signal)
    if (signal.aborted) throw new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_CANCELLED')
    const responseError = validatePdfOcrResponse(ocrRequest, response)
    if (responseError !== null) throw new PdfReconstructionWorkerError(responseError)
    return {
      ...request,
      ocrCandidates: [...request.ocrCandidates, ...response.candidates],
    }
  }

  private isCurrent(generation: number, requestId: string): boolean {
    return (
      this.generation === generation &&
      this.activeRequest?.generation === generation &&
      this.activeRequest.requestId === requestId
    )
  }

  private report(requestId: string, code: string): void {
    this.options.onDiagnostic?.({ requestId, code })
  }

  private finishWithError(code: string): void {
    this.phase = 'error'
    this.pendingRequestId = null
    this.errorValue = code
    this.notify()
  }

  private notify(): void {
    for (const listener of this.listeners) listener()
  }
}

interface RustPdfReconstructionResponse {
  readonly protocolVersion: number
  readonly requestId: string
  readonly ok: boolean
  readonly result: { readonly result: PdfReconstructionResultDto } | null
  readonly error: { readonly code: string } | null
}

interface RustPdfReconstructionAcceptResponse {
  readonly protocolVersion: number
  readonly requestId: string
  readonly ok: boolean
  readonly result: PdfReconstructionAcceptResultDto | null
  readonly error: { readonly code: string } | null
}

/** Adapts the Rust string-only reconstruction and review boundary. */
export function createWasmPdfReconstructionEngine(
  wasm: PdfReconstructionWasmBoundary,
): PdfReconstructionEngineAdapter {
  const run: PdfReconstructionEngine = async (request, signal) => {
    if (signal.aborted) throw new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_CANCELLED')
    const serializedResponse = wasm.reconstruct_pdf(JSON.stringify(toRustRequest(request)))
    if (signal.aborted) throw new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_CANCELLED')
    const parsed = parseRustResponse<RustPdfReconstructionResponse>(serializedResponse)
    if (!parsed.ok || parsed.result === null) {
      throw new PdfReconstructionWorkerError(
        parsed.error?.code ?? 'FLOW_PDF_RECONSTRUCTION_UNKNOWN_ERROR',
      )
    }
    const result = parsed.result.result
    const workerResult: PdfReconstructionWorkerResultDto = {
      protocolVersion: PDF_RECONSTRUCTION_PROTOCOL_VERSION,
      requestId: parsed.requestId,
      ...result,
    }
    const mismatch = validatePdfReconstructionResult(request, workerResult)
    if (mismatch !== null) throw new PdfReconstructionWorkerError(mismatch)
    if (!wasm.verify_pdf_reconstruction_response(serializedResponse)) {
      throw new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_RESULT_INVALID')
    }
    return workerResult
  }
  const accept: PdfReconstructionAcceptanceEngine = async (request, signal) => {
    if (signal.aborted) throw new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_CANCELLED')
    const serializedResponse = wasm.accept_pdf_reconstruction(
      JSON.stringify({
        protocolVersion: request.protocolVersion,
        requestId: request.requestId,
        result: request.result,
        decisions: request.decisions,
      }),
    )
    if (signal.aborted) throw new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_CANCELLED')
    const parsed = parseRustResponse<RustPdfReconstructionAcceptResponse>(serializedResponse)
    if (!parsed.ok || parsed.result === null) {
      throw new PdfReconstructionWorkerError(
        parsed.error?.code ?? 'FLOW_PDF_RECONSTRUCTION_UNKNOWN_ERROR',
      )
    }
    if (!wasm.verify_pdf_reconstruction_accept_response(serializedResponse)) {
      throw new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_CANDIDATE_INVALID')
    }
    return parsed.result
  }
  return {
    run,
    accept,
    verifyResultHash: (result) => result.resultHash.trim().length > 0,
    verifyAcceptedCandidate: (candidate) => candidate.canonicalHash.trim().length > 0,
  }
}

export function createWasmPdfReconstructionScheduler(
  wasm: PdfReconstructionWasmBoundary,
  options: Omit<PdfReconstructionSchedulerOptions, 'verifyResultHash' | 'verifyAcceptedCandidate'> & {
    readonly verifyResultHash?: PdfReconstructionSchedulerOptions['verifyResultHash']
    readonly verifyAcceptedCandidate?: PdfReconstructionSchedulerOptions['verifyAcceptedCandidate']
  } = {},
): RevisionAwarePdfReconstructionScheduler {
  const adapter = createWasmPdfReconstructionEngine(wasm)
  return new RevisionAwarePdfReconstructionScheduler(adapter.run, adapter.accept, {
    ...options,
    verifyResultHash: options.verifyResultHash ?? adapter.verifyResultHash,
    verifyAcceptedCandidate: options.verifyAcceptedCandidate ?? adapter.verifyAcceptedCandidate,
  })
}

export interface PdfReconstructionWorkerScope {
  onmessage: ((event: { readonly data: PdfReconstructionWorkerInboundMessage }) => void) | null
  postMessage(message: PdfReconstructionWorkerOutboundMessage): void
}

export interface PdfReconstructionWorkerPort {
  onmessage: ((event: { readonly data: PdfReconstructionWorkerOutboundMessage }) => void) | null
  onerror: ((event: unknown) => void) | null
  onmessageerror: ((event: unknown) => void) | null
  postMessage(message: PdfReconstructionWorkerInboundMessage): void
  terminate(): void
}

export function createWorkerPdfReconstructionScheduler(
  worker: PdfReconstructionWorkerPort,
  options: Omit<PdfReconstructionSchedulerOptions, 'verifyResultHash' | 'verifyAcceptedCandidate'> & {
    readonly verifyResultHash?: PdfReconstructionSchedulerOptions['verifyResultHash']
    readonly verifyAcceptedCandidate?: PdfReconstructionSchedulerOptions['verifyAcceptedCandidate']
  } = {},
): RevisionAwarePdfReconstructionScheduler {
  const engine = createWorkerPdfReconstructionEngine(worker)
  return new RevisionAwarePdfReconstructionScheduler(engine.run, engine.accept, {
    ...options,
    verifyResultHash: options.verifyResultHash ?? ((result) => result.resultHash.trim().length > 0),
    verifyAcceptedCandidate:
      options.verifyAcceptedCandidate ?? ((candidate) => candidate.canonicalHash.trim().length > 0),
  })
}

function createWorkerPdfReconstructionEngine(
  worker: PdfReconstructionWorkerPort,
): PdfReconstructionEngineAdapter {
  const pending = new Map<
    string,
    {
      readonly resolve: (value: PdfReconstructionWorkerResultDto | PdfReconstructionAcceptResultDto) => void
      readonly reject: (error: unknown) => void
    }
  >()
  let closed = false

  worker.onmessage = (event) => {
    const message = event.data
    if (message.type === 'accepted') {
      pending.get(message.requestId)?.resolve(message.result)
      pending.delete(message.requestId)
    } else if (message.type === 'acceptedCandidate') {
      pending.get(message.requestId)?.resolve(message.candidate.result)
      pending.delete(message.requestId)
    } else if (message.type === 'discarded' || message.type === 'failed') {
      pending.get(message.requestId)?.reject(new PdfReconstructionWorkerError(message.code))
      pending.delete(message.requestId)
    } else {
      pending.get(message.requestId)?.reject(
        new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_CANCELLED'),
      )
      pending.delete(message.requestId)
    }
  }
  const fail = () => {
    closed = true
    for (const entry of pending.values()) {
      entry.reject(new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_WORKER_FAILURE'))
    }
    pending.clear()
  }
  worker.onerror = fail
  worker.onmessageerror = fail

  const run: PdfReconstructionEngine = (request, signal) =>
    new Promise<PdfReconstructionWorkerResultDto>((resolve, reject) => {
      if (closed) {
        reject(new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_WORKER_FAILURE'))
        return
      }
      if (signal.aborted) {
        reject(new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_CANCELLED'))
        return
      }
      const cancel = () => {
        pending.delete(request.requestId)
        worker.postMessage({ type: 'cancel', requestId: request.requestId })
        reject(new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_CANCELLED'))
      }
      signal.addEventListener('abort', cancel, { once: true })
      pending.set(request.requestId, {
        resolve: (result) => {
          signal.removeEventListener('abort', cancel)
          resolve(result as PdfReconstructionWorkerResultDto)
        },
        reject: (error) => {
          signal.removeEventListener('abort', cancel)
          reject(error)
        },
      })
      try {
        worker.postMessage({ type: 'reconstruct', request })
      } catch (error: unknown) {
        pending.delete(request.requestId)
        signal.removeEventListener('abort', cancel)
        reject(error)
      }
    })

  const accept: PdfReconstructionAcceptanceEngine = (request, signal) =>
    new Promise<PdfReconstructionAcceptResultDto>((resolve, reject) => {
      if (closed) {
        reject(new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_WORKER_FAILURE'))
        return
      }
      if (signal.aborted) {
        reject(new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_CANCELLED'))
        return
      }
      const cancel = () => {
        pending.delete(request.requestId)
        worker.postMessage({ type: 'cancel', requestId: request.requestId })
        reject(new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_CANCELLED'))
      }
      signal.addEventListener('abort', cancel, { once: true })
      pending.set(request.requestId, {
        resolve: (result) => {
          signal.removeEventListener('abort', cancel)
          resolve(result as PdfReconstructionAcceptResultDto)
        },
        reject: (error) => {
          signal.removeEventListener('abort', cancel)
          reject(error)
        },
      })
      try {
        worker.postMessage({
          type: 'accept',
          requestId: request.requestId,
          sourceRequestId: request.sourceRequestId,
          decisions: request.decisions,
        })
      } catch (error: unknown) {
        pending.delete(request.requestId)
        signal.removeEventListener('abort', cancel)
        reject(error)
      }
    })

  return {
    run,
    accept,
    verifyResultHash: (result) => result.resultHash.trim().length > 0,
    verifyAcceptedCandidate: (candidate) => candidate.canonicalHash.trim().length > 0,
  }
}

export function installPdfReconstructionWorker(
  scope: PdfReconstructionWorkerScope,
  adapter: PdfReconstructionEngineAdapter,
): RevisionAwarePdfReconstructionScheduler {
  const scheduler = new RevisionAwarePdfReconstructionScheduler(adapter.run, adapter.accept, {
    verifyResultHash: adapter.verifyResultHash,
    verifyAcceptedCandidate: adapter.verifyAcceptedCandidate,
    onPublished: ({ request, result }) => {
      scope.postMessage({ type: 'accepted', requestId: request.requestId, result })
    },
    onAcceptedCandidate: (accepted) => {
      scope.postMessage({
        type: 'acceptedCandidate',
        requestId: accepted.request.requestId,
        candidate: accepted,
      })
    },
  })
  scope.onmessage = (event) => {
    if (event.data.type === 'cancel') {
      scheduler.cancel(event.data.requestId)
      scope.postMessage({ type: 'cancelled', requestId: event.data.requestId })
      return
    }
    if (event.data.type === 'accept') {
      void scheduler
        .accept(event.data.decisions, {
          requestId: event.data.requestId,
          sourceRequestId: event.data.sourceRequestId,
        })
        .then((outcome) => {
          if (outcome.kind === 'failed') {
            scope.postMessage({
              type: 'failed',
              requestId: outcome.requestId,
              code: outcome.code,
            })
          } else if (outcome.kind === 'cancelled') {
            scope.postMessage({ type: 'cancelled', requestId: outcome.requestId })
          }
        })
      return
    }
    void scheduler.request(event.data.request).then((outcome) => {
      if (outcome.kind === 'discarded' || outcome.kind === 'failed') {
        scope.postMessage({
          type: outcome.kind,
          requestId: outcome.requestId,
          code: outcome.code,
        })
      } else if (outcome.kind === 'cancelled') {
        scope.postMessage({ type: 'cancelled', requestId: outcome.requestId })
      }
    })
  }
  return scheduler
}

function toRustRequest(request: PdfReconstructionRequestDto): object {
  return {
    protocolVersion: request.protocolVersion,
    requestId: request.requestId,
    sourceHash: request.sourceHash,
    scene: request.scene,
    locale: request.locale,
    ocrCandidates: request.ocrCandidates,
  }
}

function parseRustResponse<T>(serializedResponse: string): T {
  let parsed: unknown
  try {
    parsed = JSON.parse(serializedResponse) as unknown
  } catch {
    throw new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_RESPONSE_DECODE')
  }
  if (
    !isRecord(parsed) ||
    typeof parsed.protocolVersion !== 'number' ||
    typeof parsed.requestId !== 'string' ||
    typeof parsed.ok !== 'boolean' ||
    (parsed.result !== null && !isRecord(parsed.result)) ||
    (parsed.error !== null &&
      (!isRecord(parsed.error) || typeof parsed.error.code !== 'string'))
  ) {
    throw new PdfReconstructionWorkerError('FLOW_PDF_RECONSTRUCTION_RESPONSE_DECODE')
  }
  return parsed as T
}

function errorCode(error: unknown): string {
  if (error instanceof PdfReconstructionWorkerError) return error.code
  if (error instanceof Error && error.message.startsWith('FLOW_')) return error.message
  return 'FLOW_PDF_RECONSTRUCTION_WORKER_FAILURE'
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}
