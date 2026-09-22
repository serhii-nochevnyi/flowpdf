import {
  PDF_READER_PROTOCOL_VERSION,
  type AcceptedPdfReaderDto,
  type PdfReaderRequestDto,
  type PdfReaderResultDto,
  type PdfReaderSchedulerSnapshotDto,
  type PdfReaderWasmBoundary,
  type PdfReaderWorkerInboundMessage,
  type PdfReaderWorkerOutboundMessage,
  type PdfReaderWorkerResultDto,
  validatePdfReaderRequest,
  validatePdfReaderResult,
} from './pdf-protocol.js'

export interface PdfReaderEngine {
  (request: PdfReaderRequestDto, signal: AbortSignal): Promise<PdfReaderWorkerResultDto>
}

export interface PdfReaderEngineAdapter {
  readonly run: PdfReaderEngine
  readonly verifyResultHash: (result: PdfReaderWorkerResultDto) => boolean | Promise<boolean>
}

export interface PdfReaderSchedulerOptions {
  readonly verifyResultHash: (
    result: PdfReaderWorkerResultDto,
  ) => boolean | Promise<boolean>
  readonly onPublished?: (accepted: AcceptedPdfReaderDto) => void
  readonly onDiagnostic?: (diagnostic: { readonly requestId: string; readonly code: string }) => void
}

export type PdfReaderScheduleOutcome =
  | { readonly kind: 'published'; readonly accepted: AcceptedPdfReaderDto }
  | { readonly kind: 'discarded'; readonly requestId: string; readonly code: string }
  | { readonly kind: 'cancelled'; readonly requestId: string }
  | { readonly kind: 'failed'; readonly requestId: string; readonly code: string }

export class PdfReaderWorkerError extends Error {
  constructor(readonly code: string) {
    super(code)
    this.name = 'PdfReaderWorkerError'
  }
}

interface ActiveRequest {
  readonly generation: number
  readonly requestId: string
  readonly controller: AbortController
}

/** Single-flight page-window scheduler with stale-result protection. */
export class RevisionAwarePdfReaderScheduler {
  private readonly listeners = new Set<() => void>()
  private generation = 0
  private activeRequest: ActiveRequest | null = null
  private acceptedValue: AcceptedPdfReaderDto | null = null
  private phase: PdfReaderSchedulerSnapshotDto['phase'] = 'idle'
  private pendingRequestId: string | null = null
  private errorValue: string | null = null

  constructor(
    private readonly engine: PdfReaderEngine,
    private readonly options: PdfReaderSchedulerOptions,
  ) {}

  accepted(): AcceptedPdfReaderDto | null {
    return this.acceptedValue
  }

  snapshot(): PdfReaderSchedulerSnapshotDto {
    return {
      phase: this.phase,
      accepted: this.acceptedValue,
      requestId: this.pendingRequestId,
      errorCode: this.errorValue,
    }
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  request(request: PdfReaderRequestDto): Promise<PdfReaderScheduleOutcome> {
    const requestError = validatePdfReaderRequest(request)
    if (requestError !== null) {
      this.phase = 'error'
      this.pendingRequestId = request.requestId
      this.errorValue = requestError
      this.notify()
      this.report(request, requestError)
      return Promise.resolve({ kind: 'failed', requestId: request.requestId, code: requestError })
    }

    this.activeRequest?.controller.abort()
    const generation = this.generation + 1
    this.generation = generation
    const controller = new AbortController()
    this.activeRequest = { generation, requestId: request.requestId, controller }
    this.phase = 'pending'
    this.pendingRequestId = request.requestId
    this.errorValue = null
    this.notify()
    return this.run(request, generation, controller)
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
    request: PdfReaderRequestDto,
    generation: number,
    controller: AbortController,
  ): Promise<PdfReaderScheduleOutcome> {
    try {
      const result = await this.engine(request, controller.signal)
      if (controller.signal.aborted) return { kind: 'cancelled', requestId: request.requestId }
      if (!this.isCurrent(generation, request.requestId)) {
        return {
          kind: 'discarded',
          requestId: request.requestId,
          code: 'FLOW_PDF_READER_STALE_REQUEST',
        }
      }
      const mismatch = validatePdfReaderResult(request, result)
      if (mismatch !== null) {
        this.finishWithError(mismatch)
        this.report(request, mismatch)
        return { kind: 'discarded', requestId: request.requestId, code: mismatch }
      }
      if (!(await this.options.verifyResultHash(result))) {
        this.finishWithError('FLOW_PDF_READER_RESULT_HASH_INVALID')
        this.report(request, 'FLOW_PDF_READER_RESULT_HASH_INVALID')
        return {
          kind: 'discarded',
          requestId: request.requestId,
          code: 'FLOW_PDF_READER_RESULT_HASH_INVALID',
        }
      }
      const accepted = Object.freeze({ request, result })
      this.acceptedValue = accepted
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
      this.report(request, code)
      return { kind: 'failed', requestId: request.requestId, code }
    } finally {
      if (this.isCurrent(generation, request.requestId)) this.activeRequest = null
    }
  }

  private isCurrent(generation: number, requestId: string): boolean {
    return (
      this.generation === generation &&
      this.activeRequest?.generation === generation &&
      this.activeRequest.requestId === requestId
    )
  }

  private report(request: PdfReaderRequestDto, code: string): void {
    this.options.onDiagnostic?.({ requestId: request.requestId, code })
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

interface RustPdfReaderResponse {
  readonly protocolVersion: number
  readonly requestId: string
  readonly ok: boolean
  readonly result: PdfReaderResultDto | null
  readonly error: { readonly code: string } | null
}

/** Adapts the Rust string-only reader boundary to a cancellable engine. */
export function createWasmPdfReaderEngine(wasm: PdfReaderWasmBoundary): PdfReaderEngineAdapter {
  const run: PdfReaderEngine = async (request, signal) => {
    if (signal.aborted) throw new PdfReaderWorkerError('FLOW_PDF_READER_CANCELLED')
    const serializedResponse = wasm.read_pdf(JSON.stringify(request))
    if (signal.aborted) throw new PdfReaderWorkerError('FLOW_PDF_READER_CANCELLED')
    let parsed: unknown
    try {
      parsed = JSON.parse(serializedResponse) as unknown
    } catch {
      throw new PdfReaderWorkerError('FLOW_PDF_READER_RESPONSE_DECODE')
    }
    if (!isRustPdfReaderResponse(parsed)) {
      throw new PdfReaderWorkerError('FLOW_PDF_READER_RESPONSE_DECODE')
    }
    if (!parsed.ok || parsed.result === null) {
      throw new PdfReaderWorkerError(parsed.error?.code ?? 'FLOW_PDF_READER_UNKNOWN_ERROR')
    }
    if (!isPdfReaderResult(parsed.result)) {
      throw new PdfReaderWorkerError('FLOW_PDF_READER_RESPONSE_DECODE')
    }
    if (!wasm.verify_pdf_reader_response(serializedResponse)) {
      throw new PdfReaderWorkerError('FLOW_PDF_READER_RESULT_HASH_INVALID')
    }
    return {
      protocolVersion: PDF_READER_PROTOCOL_VERSION,
      requestId: parsed.requestId,
      ...parsed.result,
    }
  }
  return {
    run,
    verifyResultHash: (result) => result.resultHash.trim().length > 0,
  }
}

export function createWasmPdfReaderScheduler(
  wasm: PdfReaderWasmBoundary,
  options: Omit<PdfReaderSchedulerOptions, 'verifyResultHash'> = {},
): RevisionAwarePdfReaderScheduler {
  const adapter = createWasmPdfReaderEngine(wasm)
  return new RevisionAwarePdfReaderScheduler(adapter.run, {
    ...options,
    verifyResultHash: adapter.verifyResultHash,
  })
}

export interface PdfReaderWorkerScope {
  onmessage: ((event: { readonly data: PdfReaderWorkerInboundMessage }) => void) | null
  postMessage(message: PdfReaderWorkerOutboundMessage): void
}

export interface PdfReaderWorkerPort {
  onmessage: ((event: { readonly data: PdfReaderWorkerOutboundMessage }) => void) | null
  onerror: ((event: unknown) => void) | null
  onmessageerror: ((event: unknown) => void) | null
  postMessage(message: PdfReaderWorkerInboundMessage): void
  terminate(): void
}

export function createWorkerPdfReaderScheduler(
  worker: PdfReaderWorkerPort,
  options: Omit<PdfReaderSchedulerOptions, 'verifyResultHash'> & {
    readonly verifyResultHash?: PdfReaderSchedulerOptions['verifyResultHash']
  } = {},
): RevisionAwarePdfReaderScheduler {
  const engine = createWorkerPdfReaderEngine(worker)
  return new RevisionAwarePdfReaderScheduler(engine, {
    ...options,
    verifyResultHash: options.verifyResultHash ?? ((result) => result.resultHash.trim().length > 0),
  })
}

function createWorkerPdfReaderEngine(worker: PdfReaderWorkerPort): PdfReaderEngine {
  const pending = new Map<
    string,
    {
      readonly resolve: (result: PdfReaderWorkerResultDto) => void
      readonly reject: (error: unknown) => void
    }
  >()
  let closed = false

  worker.onmessage = (event) => {
    const message = event.data
    if (message.type === 'accepted') {
      pending.get(message.requestId)?.resolve(message.result)
      pending.delete(message.requestId)
    } else if (message.type === 'discarded' || message.type === 'failed') {
      pending.get(message.requestId)?.reject(new PdfReaderWorkerError(message.code))
      pending.delete(message.requestId)
    } else {
      pending.get(message.requestId)?.reject(new PdfReaderWorkerError('FLOW_PDF_READER_CANCELLED'))
      pending.delete(message.requestId)
    }
  }
  const fail = () => {
    closed = true
    for (const entry of pending.values()) {
      entry.reject(new PdfReaderWorkerError('FLOW_PDF_READER_WORKER_FAILURE'))
    }
    pending.clear()
  }
  worker.onerror = fail
  worker.onmessageerror = fail

  return (request, signal) => {
    if (closed) return Promise.reject(new PdfReaderWorkerError('FLOW_PDF_READER_WORKER_FAILURE'))
    if (signal.aborted) return Promise.reject(new PdfReaderWorkerError('FLOW_PDF_READER_CANCELLED'))
    return new Promise<PdfReaderWorkerResultDto>((resolve, reject) => {
      const cancel = () => {
        pending.delete(request.requestId)
        worker.postMessage({ type: 'cancel', requestId: request.requestId })
        reject(new PdfReaderWorkerError('FLOW_PDF_READER_CANCELLED'))
      }
      signal.addEventListener('abort', cancel, { once: true })
      pending.set(request.requestId, {
        resolve: (result) => {
          signal.removeEventListener('abort', cancel)
          resolve(result)
        },
        reject: (error) => {
          signal.removeEventListener('abort', cancel)
          reject(error)
        },
      })
      try {
        worker.postMessage({ type: 'read', request })
      } catch (error: unknown) {
        pending.delete(request.requestId)
        signal.removeEventListener('abort', cancel)
        reject(error)
      }
    })
  }
}

export function installPdfReaderWorker(
  scope: PdfReaderWorkerScope,
  engine: PdfReaderEngine,
  verifyResultHash: PdfReaderSchedulerOptions['verifyResultHash'],
): RevisionAwarePdfReaderScheduler {
  const scheduler = new RevisionAwarePdfReaderScheduler(engine, {
    verifyResultHash,
    onPublished: ({ request, result }) => {
      scope.postMessage({ type: 'accepted', requestId: request.requestId, result })
    },
  })
  scope.onmessage = (event) => {
    if (event.data.type === 'cancel') {
      scheduler.cancel(event.data.requestId)
      scope.postMessage({ type: 'cancelled', requestId: event.data.requestId })
      return
    }
    void scheduler.request(event.data.request).then((outcome) => {
      if (outcome.kind === 'discarded') {
        scope.postMessage({ type: 'discarded', requestId: outcome.requestId, code: outcome.code })
      } else if (outcome.kind === 'cancelled') {
        scope.postMessage({ type: 'cancelled', requestId: outcome.requestId })
      } else if (outcome.kind === 'failed') {
        scope.postMessage({ type: 'failed', requestId: outcome.requestId, code: outcome.code })
      }
    })
  }
  return scheduler
}

function errorCode(error: unknown): string {
  if (error instanceof PdfReaderWorkerError) return error.code
  return 'FLOW_PDF_READER_WORKER_FAILURE'
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function isRustPdfReaderResponse(value: unknown): value is RustPdfReaderResponse {
  if (!isRecord(value)) return false
  if (
    typeof value.protocolVersion !== 'number' ||
    typeof value.requestId !== 'string' ||
    typeof value.ok !== 'boolean' ||
    (value.result !== null && !isRecord(value.result))
  ) {
    return false
  }
  if (value.error !== null && !isRecord(value.error)) return false
  if (value.error !== null && typeof value.error.code !== 'string') return false
  return true
}

function isPdfReaderResult(value: PdfReaderResultDto): value is PdfReaderResultDto {
  if (!isRecord(value) || !isRecord(value.summary) || !isRecord(value.scene)) return false
  if (
    typeof value.resultHash !== 'string' ||
    typeof value.summary.schemaVersion !== 'number' ||
    typeof value.summary.pdfVersion !== 'string' ||
    typeof value.summary.sourceHash !== 'string' ||
    typeof value.summary.pageCount !== 'number' ||
    typeof value.scene.schemaVersion !== 'number' ||
    typeof value.scene.sourceHash !== 'string' ||
    typeof value.scene.pageCount !== 'number' ||
    typeof value.scene.firstPage !== 'number' ||
    typeof value.scene.resultHash !== 'string' ||
    !Array.isArray(value.scene.pages) ||
    !isReaderReport(value.scene.report)
  ) {
    return false
  }
  return value.scene.pages.every((page) => {
    if (!isRecord(page) || !isRecord(page.bounds) || !isReaderReport(page.report)) return false
    return (
      typeof page.pageIndex === 'number' &&
      Array.isArray(page.elements) &&
      typeof page.partial === 'boolean'
    )
  })
}

function isReaderReport(value: unknown): boolean {
  if (!isRecord(value) || typeof value.partial !== 'boolean' || !Array.isArray(value.diagnostics)) {
    return false
  }
  return value.diagnostics.every((diagnostic) => {
    if (!isRecord(diagnostic)) return false
    return typeof diagnostic.code === 'string' && typeof diagnostic.severity === 'string'
  })
}
