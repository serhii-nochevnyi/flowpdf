import {
  LAYOUT_PROTOCOL_VERSION,
  type AcceptedLayoutDto,
  type LayoutRequestDto,
  type LayoutResultDto,
  type LayoutWorkerDiagnosticDto,
  type LayoutWorkerInboundMessage,
  type LayoutWorkerOutboundMessage,
  type LayoutWorkerResultDto,
  validateLayoutRequest,
  validateLayoutResult,
} from './layout-protocol.js'

export interface LayoutEngine {
  (request: LayoutRequestDto, signal: AbortSignal): Promise<LayoutWorkerResultDto>
}

export interface LayoutWasmBoundary {
  readonly layout_document: (requestJson: string) => string
  readonly verify_layout_response: (responseJson: string) => boolean
}

export interface LayoutEngineAdapter {
  readonly run: LayoutEngine
  /**
   * Rust verifies the complete serialized response before this adapter maps
   * it to a worker result. The scheduler calls this second guard immediately
   * before publication so a mutated/f forged result cannot enter the store.
   */
  readonly verifyResultHash: (result: LayoutWorkerResultDto) => boolean | Promise<boolean>
}

export interface LayoutSchedulerOptions {
  readonly verifyResultHash: (
    result: LayoutWorkerResultDto,
  ) => boolean | Promise<boolean>
  readonly onPublished?: (accepted: AcceptedLayoutDto) => void
  readonly onDiagnostic?: (diagnostic: LayoutWorkerDiagnosticDto) => void
}

export type LayoutScheduleOutcome =
  | { readonly kind: 'published'; readonly accepted: AcceptedLayoutDto }
  | { readonly kind: 'discarded'; readonly requestId: string; readonly code: string }
  | { readonly kind: 'cancelled'; readonly requestId: string }
  | { readonly kind: 'failed'; readonly requestId: string; readonly code: string }

export class LayoutWorkerError extends Error {
  constructor(readonly code: string) {
    super(code)
    this.name = 'LayoutWorkerError'
  }
}

interface ActiveRequest {
  readonly generation: number
  readonly requestId: string
  readonly controller: AbortController
}

/**
 * Revision-aware single-flight scheduler. It never mutates the accepted
 * result until a complete response has passed every identity and hash guard.
 */
export class RevisionAwareLayoutScheduler {
  private readonly listeners = new Set<() => void>()
  private generation = 0
  private activeRequest: ActiveRequest | null = null
  private acceptedValue: AcceptedLayoutDto | null = null

  constructor(
    private readonly engine: LayoutEngine,
    private readonly options: LayoutSchedulerOptions,
  ) {}

  accepted(): AcceptedLayoutDto | null {
    return this.acceptedValue
  }

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  request(request: LayoutRequestDto): Promise<LayoutScheduleOutcome> {
    const requestError = validateLayoutRequest(request)
    if (requestError !== null) {
      this.report(request, requestError)
      return Promise.resolve({
        kind: 'failed',
        requestId: request.requestId,
        code: requestError,
      })
    }

    this.activeRequest?.controller.abort()
    const generation = this.generation + 1
    this.generation = generation
    const controller = new AbortController()
    this.activeRequest = {
      generation,
      requestId: request.requestId,
      controller,
    }

    return this.run(request, generation, controller)
  }

  cancel(requestId?: string): void {
    const active = this.activeRequest
    if (active === null || (requestId !== undefined && active.requestId !== requestId)) return
    active.controller.abort()
    this.generation += 1
    this.activeRequest = null
  }

  dispose(): void {
    this.cancel()
    this.listeners.clear()
  }

  private async run(
    request: LayoutRequestDto,
    generation: number,
    controller: AbortController,
  ): Promise<LayoutScheduleOutcome> {
    try {
      const result = await this.engine(request, controller.signal)
      if (controller.signal.aborted) {
        return { kind: 'cancelled', requestId: request.requestId }
      }
      if (!this.isCurrent(generation, request.requestId)) {
        return {
          kind: 'discarded',
          requestId: request.requestId,
          code: 'FLOW_LAYOUT_STALE_REQUEST',
        }
      }

      const mismatch = validateLayoutResult(request, result)
      if (mismatch !== null) {
        this.report(request, mismatch)
        return { kind: 'discarded', requestId: request.requestId, code: mismatch }
      }
      if (!(await this.options.verifyResultHash(result))) {
        this.report(request, 'FLOW_LAYOUT_RESULT_HASH_INVALID')
        return {
          kind: 'discarded',
          requestId: request.requestId,
          code: 'FLOW_LAYOUT_RESULT_HASH_INVALID',
        }
      }

      // The complete result is installed as one immutable projection. No page
      // or fragment is visible to consumers before this point.
      const accepted = Object.freeze({ request, result })
      this.acceptedValue = accepted
      for (const listener of this.listeners) listener()
      this.options.onPublished?.(accepted)
      return { kind: 'published', accepted }
    } catch (error: unknown) {
      if (controller.signal.aborted) {
        return { kind: 'cancelled', requestId: request.requestId }
      }
      const code = errorCode(error)
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

  private report(request: LayoutRequestDto, code: string): void {
    this.options.onDiagnostic?.({
      requestId: request.requestId,
      sourceRevision: request.sourceRevision,
      code,
    })
  }
}

interface RustLayoutResponse {
  readonly schemaVersion: number
  readonly requestId: string
  readonly ok: boolean
  readonly result: LayoutResultDto | null
  readonly error: { readonly code: string } | null
}

/** Adapts the string-only WASM boundary to the revision-aware scheduler. */
export function createWasmLayoutEngine(wasm: LayoutWasmBoundary): LayoutEngineAdapter {
  const run: LayoutEngine = async (request, signal) => {
    if (signal.aborted) throw new LayoutWorkerError('FLOW_LAYOUT_CANCELLED')
    const serializedResponse = wasm.layout_document(request.serializedRequest)
    if (signal.aborted) throw new LayoutWorkerError('FLOW_LAYOUT_CANCELLED')
    let parsed: unknown
    try {
      parsed = JSON.parse(serializedResponse) as unknown
    } catch {
      throw new LayoutWorkerError('FLOW_LAYOUT_RESPONSE_DECODE')
    }
    if (!isRustLayoutResponse(parsed)) {
      throw new LayoutWorkerError('FLOW_LAYOUT_RESPONSE_DECODE')
    }
    if (!parsed.ok || parsed.result === null) {
      throw new LayoutWorkerError(parsed.error?.code ?? 'FLOW_LAYOUT_UNKNOWN_ERROR')
    }
    if (!wasm.verify_layout_response(serializedResponse)) {
      throw new LayoutWorkerError('FLOW_LAYOUT_RESULT_HASH_INVALID')
    }
    return {
      protocolVersion: LAYOUT_PROTOCOL_VERSION,
      requestId: parsed.requestId,
      ...parsed.result,
    }
  }

  return {
    run,
    verifyResultHash: (result) => result.resultHash.trim().length > 0,
  }
}

export interface LayoutWorkerScope {
  onmessage: ((event: { readonly data: LayoutWorkerInboundMessage }) => void) | null
  postMessage(message: LayoutWorkerOutboundMessage): void
}

/** Installs the message loop used by a dedicated worker entry point. */
export function installLayoutWorker(
  scope: LayoutWorkerScope,
  engine: LayoutEngine,
  verifyResultHash: LayoutSchedulerOptions['verifyResultHash'],
): RevisionAwareLayoutScheduler {
  const scheduler = new RevisionAwareLayoutScheduler(engine, {
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
        scope.postMessage({
          type: 'discarded',
          requestId: outcome.requestId,
          code: outcome.code,
        })
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
  if (error instanceof LayoutWorkerError) return error.code
  return 'FLOW_LAYOUT_WORKER_FAILURE'
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function isRustLayoutResponse(value: unknown): value is RustLayoutResponse {
  if (!isRecord(value)) return false
  if (
    typeof value.schemaVersion !== 'number' ||
    typeof value.requestId !== 'string' ||
    typeof value.ok !== 'boolean' ||
    (value.result !== null && !isRecord(value.result))
  ) {
    return false
  }
  if (value.error !== null && !isRecord(value.error)) return false
  if (value.error !== null && typeof value.error.code !== 'string') return false
  if (value.result === null) return true
  return (
    typeof value.result.sourceRevision === 'number' &&
    typeof value.result.sourceHash === 'string' &&
    typeof value.result.layoutSettingsFingerprint === 'string' &&
    typeof value.result.fontCatalogIdentity === 'string' &&
    (value.result.hyphenationDataIdentity === null ||
      typeof value.result.hyphenationDataIdentity === 'string') &&
    Array.isArray(value.result.pages) &&
    Array.isArray(value.result.diagnostics) &&
    typeof value.result.resultHash === 'string'
  )
}
