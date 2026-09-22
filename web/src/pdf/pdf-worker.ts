import {
  PDF_PROTOCOL_VERSION,
  type AcceptedPdfExportDto,
  type PdfExportRequestDto,
  type PdfExportResultDto,
  type PdfExportSchedulerSnapshotDto,
  type PdfExportWorkerResultDto,
  type PdfRecoveryRequestDto,
  type PdfRecoveryResultDto,
  type PdfWorkerDiagnosticDto,
  type PdfWorkerInboundMessage,
  type PdfWorkerOutboundMessage,
  type PdfWasmBoundary,
  validatePdfExportRequest,
  validatePdfExportResult,
} from './pdf-protocol.js'

export interface PdfExportEngine {
  (request: PdfExportRequestDto, signal: AbortSignal): Promise<PdfExportWorkerResultDto>
}

export interface PdfExportEngineAdapter {
  readonly run: PdfExportEngine
  readonly verifyResultHash: (
    result: PdfExportWorkerResultDto,
  ) => boolean | Promise<boolean>
}

export interface PdfExportSchedulerOptions {
  readonly verifyResultHash: (
    result: PdfExportWorkerResultDto,
  ) => boolean | Promise<boolean>
  readonly onPublished?: (accepted: AcceptedPdfExportDto) => void
  readonly onDiagnostic?: (diagnostic: PdfWorkerDiagnosticDto) => void
}

export type PdfExportScheduleOutcome =
  | { readonly kind: 'published'; readonly accepted: AcceptedPdfExportDto }
  | { readonly kind: 'discarded'; readonly requestId: string; readonly code: string }
  | { readonly kind: 'cancelled'; readonly requestId: string }
  | { readonly kind: 'failed'; readonly requestId: string; readonly code: string }

export class PdfWorkerError extends Error {
  constructor(readonly code: string) {
    super(code)
    this.name = 'PdfWorkerError'
  }
}

interface ActiveRequest {
  readonly generation: number
  readonly requestId: string
  readonly controller: AbortController
}

/** Single-flight revision-safe export scheduler. */
export class RevisionAwarePdfExportScheduler {
  private readonly listeners = new Set<() => void>()
  private generation = 0
  private activeRequest: ActiveRequest | null = null
  private acceptedValue: AcceptedPdfExportDto | null = null
  private phase: PdfExportSchedulerSnapshotDto['phase'] = 'idle'
  private pendingRequestId: string | null = null
  private errorValue: string | null = null

  constructor(
    private readonly engine: PdfExportEngine,
    private readonly options: PdfExportSchedulerOptions,
  ) {}

  accepted(): AcceptedPdfExportDto | null {
    return this.acceptedValue
  }

  snapshot(): PdfExportSchedulerSnapshotDto {
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

  request(request: PdfExportRequestDto): Promise<PdfExportScheduleOutcome> {
    const requestError = validatePdfExportRequest(request)
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
    request: PdfExportRequestDto,
    generation: number,
    controller: AbortController,
  ): Promise<PdfExportScheduleOutcome> {
    try {
      const result = await this.engine(request, controller.signal)
      if (controller.signal.aborted) return { kind: 'cancelled', requestId: request.requestId }
      if (!this.isCurrent(generation, request.requestId)) {
        return {
          kind: 'discarded',
          requestId: request.requestId,
          code: 'FLOW_PDF_STALE_REQUEST',
        }
      }
      const mismatch = validatePdfExportResult(request, result)
      if (mismatch !== null) {
        this.finishWithError(mismatch)
        this.report(request, mismatch)
        return { kind: 'discarded', requestId: request.requestId, code: mismatch }
      }
      if (!(await this.options.verifyResultHash(result))) {
        this.finishWithError('FLOW_PDF_BYTE_HASH_INVALID')
        this.report(request, 'FLOW_PDF_BYTE_HASH_INVALID')
        return {
          kind: 'discarded',
          requestId: request.requestId,
          code: 'FLOW_PDF_BYTE_HASH_INVALID',
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

  private report(request: PdfExportRequestDto, code: string): void {
    this.options.onDiagnostic?.({
      requestId: request.requestId,
      sourceRevision: request.sourceRevision,
      code,
    })
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

interface RustPdfResponse<T> {
  readonly protocolVersion: number
  readonly requestId: string
  readonly ok: boolean
  readonly result: T | null
  readonly error: { readonly code: string } | null
}

/** Adapts Rust's string-only export boundary to the scheduler. */
export function createWasmPdfExportEngine(wasm: PdfWasmBoundary): PdfExportEngineAdapter {
  const run: PdfExportEngine = async (request, signal) => {
    if (signal.aborted) throw new PdfWorkerError('FLOW_PDF_CANCELLED')
    const serializedResponse = wasm.export_pdf(request.serializedRequest)
    if (signal.aborted) throw new PdfWorkerError('FLOW_PDF_CANCELLED')
    let parsed: unknown
    try {
      parsed = JSON.parse(serializedResponse) as unknown
    } catch {
      throw new PdfWorkerError('FLOW_PDF_RESPONSE_DECODE')
    }
    if (!isRustPdfResponse(parsed)) throw new PdfWorkerError('FLOW_PDF_RESPONSE_DECODE')
    if (!parsed.ok || parsed.result === null) {
      throw new PdfWorkerError(parsed.error?.code ?? 'FLOW_PDF_UNKNOWN_ERROR')
    }
    if (!isPdfExportResult(parsed.result)) {
      throw new PdfWorkerError('FLOW_PDF_RESPONSE_DECODE')
    }
    if (!wasm.verify_pdf_export_response(serializedResponse)) {
      throw new PdfWorkerError('FLOW_PDF_BYTE_HASH_INVALID')
    }
    return {
      protocolVersion: PDF_PROTOCOL_VERSION,
      requestId: parsed.requestId,
      ...parsed.result,
    }
  }
  return { run, verifyResultHash: (result) => result.byteHash.trim().length > 0 }
}

/** Composes the string-only WASM adapter with the revision-aware scheduler. */
export function createWasmPdfExportScheduler(
  wasm: PdfWasmBoundary,
  options: Omit<PdfExportSchedulerOptions, 'verifyResultHash'> = {},
): RevisionAwarePdfExportScheduler {
  const adapter = createWasmPdfExportEngine(wasm)
  return new RevisionAwarePdfExportScheduler(adapter.run, {
    ...options,
    verifyResultHash: adapter.verifyResultHash,
  })
}

/** Parses exact recovery responses without treating errors as source data. */
export function recoverWithWasm(
  wasm: PdfWasmBoundary,
  request: PdfRecoveryRequestDto,
): PdfRecoveryResultDto {
  const serialized = wasm.recover_owned_source(JSON.stringify(request))
  let parsed: unknown
  try {
    parsed = JSON.parse(serialized) as unknown
  } catch {
    throw new PdfWorkerError('FLOW_PDF_RECOVERY_RESPONSE_DECODE')
  }
  if (!isRustPdfResponse<PdfRecoveryResultDto>(parsed)) {
    throw new PdfWorkerError('FLOW_PDF_RECOVERY_RESPONSE_DECODE')
  }
  if (!parsed.ok || parsed.result === null) {
    throw new PdfWorkerError(parsed.error?.code ?? 'FLOW_PDF_RECOVERY_UNKNOWN_ERROR')
  }
  return parsed.result
}

export interface PdfWorkerScope {
  onmessage: ((event: { readonly data: PdfWorkerInboundMessage }) => void) | null
  postMessage(message: PdfWorkerOutboundMessage): void
}

export interface PdfWorkerPort {
  onmessage: ((event: { readonly data: PdfWorkerOutboundMessage }) => void) | null
  onerror: ((event: unknown) => void) | null
  onmessageerror: ((event: unknown) => void) | null
  postMessage(message: PdfWorkerInboundMessage): void
  terminate(): void
}

/** Creates a scheduler whose engine delegates to an already-created Worker. */
export function createWorkerPdfExportScheduler(
  worker: PdfWorkerPort,
  options: Omit<PdfExportSchedulerOptions, 'verifyResultHash'> & {
    readonly verifyResultHash?: PdfExportSchedulerOptions['verifyResultHash']
  } = {},
): RevisionAwarePdfExportScheduler {
  const engine = createWorkerPdfExportEngine(worker)
  return new RevisionAwarePdfExportScheduler(engine, {
    ...options,
    verifyResultHash: options.verifyResultHash ?? ((result) => result.byteHash.trim().length > 0),
  })
}

function createWorkerPdfExportEngine(worker: PdfWorkerPort): PdfExportEngine {
  const pending = new Map<
    string,
    { readonly resolve: (result: PdfExportWorkerResultDto) => void; readonly reject: (error: unknown) => void }
  >()
  let closed = false

  worker.onmessage = (event) => {
    const message = event.data
    if (message.type === 'accepted') {
      pending.get(message.requestId)?.resolve(message.result)
      pending.delete(message.requestId)
    } else if (message.type === 'discarded' || message.type === 'failed') {
      pending.get(message.requestId)?.reject(new PdfWorkerError(message.code))
      pending.delete(message.requestId)
    } else {
      pending.get(message.requestId)?.reject(new PdfWorkerError('FLOW_PDF_CANCELLED'))
      pending.delete(message.requestId)
    }
  }
  const fail = () => {
    closed = true
    for (const entry of pending.values()) entry.reject(new PdfWorkerError('FLOW_PDF_WORKER_FAILURE'))
    pending.clear()
  }
  worker.onerror = fail
  worker.onmessageerror = fail

  return (request, signal) => {
    if (closed) return Promise.reject(new PdfWorkerError('FLOW_PDF_WORKER_FAILURE'))
    if (signal.aborted) return Promise.reject(new PdfWorkerError('FLOW_PDF_CANCELLED'))
    return new Promise<PdfExportWorkerResultDto>((resolve, reject) => {
      const cancel = () => {
        pending.delete(request.requestId)
        worker.postMessage({ type: 'cancel', requestId: request.requestId })
        reject(new PdfWorkerError('FLOW_PDF_CANCELLED'))
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
        worker.postMessage({ type: 'export', request })
      } catch (error: unknown) {
        pending.delete(request.requestId)
        signal.removeEventListener('abort', cancel)
        reject(error)
      }
    })
  }
}

/** Installs the message loop for a dedicated PDF worker entry point. */
export function installPdfWorker(
  scope: PdfWorkerScope,
  engine: PdfExportEngine,
  verifyResultHash: PdfExportSchedulerOptions['verifyResultHash'],
): RevisionAwarePdfExportScheduler {
  const scheduler = new RevisionAwarePdfExportScheduler(engine, {
    verifyResultHash,
    onPublished: ({ request, result }) => {
      scope.postMessage({ type: 'accepted', requestId: request.requestId, result })
    },
  })
  scope.onmessage = (event) => {
    if (event.data.type === 'cancel') {
      scheduler.cancel(event.data.requestId)
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
  if (error instanceof PdfWorkerError) return error.code
  return 'FLOW_PDF_WORKER_FAILURE'
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function isRustPdfResponse<T = PdfExportResultDto>(
  value: unknown,
): value is RustPdfResponse<T> {
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

function isPdfExportResult(value: unknown): value is PdfExportResultDto {
  if (!isRecord(value)) return false
  return (
    typeof value.sourceRevision === 'number' &&
    typeof value.sourceHash === 'string' &&
    typeof value.layoutSettingsFingerprint === 'string' &&
    typeof value.exportFingerprint === 'string' &&
    typeof value.byteHash === 'string' &&
    typeof value.bytesHex === 'string' &&
    typeof value.privateSourceStreamHex === 'string' &&
    isRecord(value.manifest) &&
    isRecord(value.supportReport)
  )
}
