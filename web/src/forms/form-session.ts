import type {
  FormSessionIdentityDto,
  FormSessionStateDto,
  FormSessionValueDto,
} from '../../persistence/indexeddb-store.js'

export const FORM_SESSION_PROTOCOL_VERSION = 1 as const

export type FormSessionActionDto =
  | { readonly type: 'start' }
  | { readonly type: 'validate' }
  | {
      readonly type: 'setValue'
      readonly fieldId: string
      readonly value: FormSessionValueDto
    }
  | { readonly type: 'clearValue'; readonly fieldId: string }

export interface FormSessionRequestDto {
  readonly protocolVersion: typeof FORM_SESSION_PROTOCOL_VERSION
  readonly canonicalJson: string
  readonly session: FormSessionStateDto | null
  readonly action: FormSessionActionDto
}

export interface FormSessionResponseDto {
  readonly protocolVersion: typeof FORM_SESSION_PROTOCOL_VERSION
  readonly session: FormSessionStateDto
}

export interface FormSessionErrorDto {
  readonly code: string
  readonly message: string
  readonly audit: unknown | null
}

export interface FormSessionApiResponse<T> {
  readonly ok: boolean
  readonly value: T | null
  readonly error: FormSessionErrorDto | null
}

export interface FormSessionWasmBoundary {
  readonly apply_form_session: (
    request: unknown,
  ) => FormSessionApiResponse<FormSessionResponseDto>
}

export interface FormSessionPersistence {
  loadFormSession(identity: FormSessionIdentityDto): Promise<FormSessionStateDto | null>
  saveFormSession(session: FormSessionStateDto, expectedGeneration: number | null): Promise<void>
}

export type FormSessionCoordinatorPhase = 'idle' | 'loading' | 'ready' | 'error'

export interface FormSessionCoordinatorSnapshot {
  readonly phase: FormSessionCoordinatorPhase
  readonly identity: FormSessionIdentityDto | null
  readonly session: FormSessionStateDto | null
  readonly errorCode: string | null
}

export interface OptionalFormSessionWasmBoundary {
  readonly apply_form_session?: FormSessionWasmBoundary['apply_form_session']
}

export class FormSessionBridgeError extends Error {
  constructor(readonly code: string) {
    super(code)
    this.name = 'FormSessionBridgeError'
  }
}

export function requireFormSessionWasm(
  boundary: OptionalFormSessionWasmBoundary,
): FormSessionWasmBoundary {
  if (boundary.apply_form_session === undefined) {
    throw new FormSessionBridgeError('FLOW_FORM_SESSION_UNAVAILABLE')
  }
  return { apply_form_session: boundary.apply_form_session }
}

/**
 * Routes noncanonical form-value actions through Rust and persists only an
 * accepted, source-bound session returned by that boundary.
 */
export class RustFormSessionBridge {
  constructor(
    private readonly wasm: FormSessionWasmBoundary,
    private readonly persistence: FormSessionPersistence,
  ) {}

  async restoreOrStart(
    canonicalJson: string,
    identity: FormSessionIdentityDto,
  ): Promise<FormSessionStateDto> {
    const existing = await this.persistence.loadFormSession(identity)
    if (existing !== null) {
      return this.run(canonicalJson, identity, existing, { type: 'validate' })
    }
    const session = this.run(canonicalJson, identity, null, { type: 'start' })
    await this.persistence.saveFormSession(session, null)
    return session
  }

  async setValue(
    canonicalJson: string,
    identity: FormSessionIdentityDto,
    session: FormSessionStateDto,
    fieldId: string,
    value: FormSessionValueDto,
  ): Promise<FormSessionStateDto> {
    const next = this.run(canonicalJson, identity, session, {
      type: 'setValue',
      fieldId,
      value,
    })
    await this.persistence.saveFormSession(next, session.generation)
    return next
  }

  async clearValue(
    canonicalJson: string,
    identity: FormSessionIdentityDto,
    session: FormSessionStateDto,
    fieldId: string,
  ): Promise<FormSessionStateDto> {
    const next = this.run(canonicalJson, identity, session, {
      type: 'clearValue',
      fieldId,
    })
    await this.persistence.saveFormSession(next, session.generation)
    return next
  }

  validate(
    canonicalJson: string,
    identity: FormSessionIdentityDto,
    session: FormSessionStateDto,
  ): FormSessionStateDto {
    return this.run(canonicalJson, identity, session, { type: 'validate' })
  }

  private run(
    canonicalJson: string,
    identity: FormSessionIdentityDto,
    session: FormSessionStateDto | null,
    action: FormSessionActionDto,
  ): FormSessionStateDto {
    let response: FormSessionApiResponse<FormSessionResponseDto>
    try {
      response = this.wasm.apply_form_session({
        protocolVersion: FORM_SESSION_PROTOCOL_VERSION,
        canonicalJson,
        session,
        action,
      })
    } catch (error: unknown) {
      if (error instanceof FormSessionBridgeError) throw error
      throw new FormSessionBridgeError('FLOW_FORM_SESSION_BOUNDARY_FAILED')
    }
    if (!response || response.ok !== true || response.value === null) {
      throw new FormSessionBridgeError(
        response?.error?.code ?? 'FLOW_UNKNOWN_CORE_ERROR',
      )
    }
    if (response.value.protocolVersion !== FORM_SESSION_PROTOCOL_VERSION) {
      throw new FormSessionBridgeError('FLOW_FORM_SESSION_PROTOCOL_UNSUPPORTED')
    }
    const returned = response.value.session
    if (!isTransportSession(returned) || !sameIdentity(returned, identity)) {
      throw new FormSessionBridgeError('FLOW_FORM_SESSION_IDENTITY_MISMATCH')
    }
    return returned
  }
}

/**
 * Owns only the browser-facing lifecycle of one source-bound session. The
 * editor controller remains the authority for canonical revisions/history.
 */
export class FormSessionCoordinator {
  private readonly bridge: Promise<RustFormSessionBridge>
  private readonly listeners = new Set<() => void>()
  private snapshotValue: FormSessionCoordinatorSnapshot = Object.freeze({
    phase: 'idle',
    identity: null,
    session: null,
    errorCode: null,
  })
  private synchronizationToken = 0
  private actionQueue: Promise<void> = Promise.resolve()

  constructor(options: {
    readonly wasm: Promise<FormSessionWasmBoundary>
    readonly persistence: FormSessionPersistence
  }) {
    this.bridge = options.wasm.then(
      (wasm) => new RustFormSessionBridge(wasm, options.persistence),
    )
  }

  readonly subscribe = (listener: () => void): (() => void) => {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  readonly getSnapshot = (): FormSessionCoordinatorSnapshot => this.snapshotValue

  async synchronize(
    canonicalJson: string,
    identity: FormSessionIdentityDto,
  ): Promise<void> {
    const current = this.snapshotValue
    if (
      sameIdentity(current.identity, identity) &&
      (current.phase === 'ready' || current.phase === 'loading')
    ) {
      return
    }
    const token = ++this.synchronizationToken
    this.publish({ phase: 'loading', identity, session: null, errorCode: null })
    try {
      const session = await (await this.bridge).restoreOrStart(canonicalJson, identity)
      if (token !== this.synchronizationToken) return
      this.publish({ phase: 'ready', identity, session, errorCode: null })
    } catch (error: unknown) {
      if (token !== this.synchronizationToken) return
      this.publish({
        phase: 'error',
        identity,
        session: null,
        errorCode: sessionErrorCode(error),
      })
    }
  }

  setValue(
    canonicalJson: string,
    identity: FormSessionIdentityDto,
    fieldId: string,
    value: FormSessionValueDto,
  ): Promise<void> {
    return this.enqueue(() => this.mutate('set', canonicalJson, identity, fieldId, value))
  }

  clearValue(
    canonicalJson: string,
    identity: FormSessionIdentityDto,
    fieldId: string,
  ): Promise<void> {
    return this.enqueue(() => this.mutate('clear', canonicalJson, identity, fieldId, null))
  }

  private enqueue(work: () => Promise<void>): Promise<void> {
    const next = this.actionQueue.then(work, work)
    this.actionQueue = next.then(
      () => undefined,
      () => undefined,
    )
    return next
  }

  private async mutate(
    operation: 'set' | 'clear',
    canonicalJson: string,
    identity: FormSessionIdentityDto,
    fieldId: string,
    value: FormSessionValueDto | null,
  ): Promise<void> {
    const current = this.snapshotValue
    if (current.session === null || !sameIdentity(current.identity, identity)) {
      const error = new FormSessionBridgeError('FLOW_FORM_SESSION_NOT_READY')
      this.publishError(identity, error.code, current.session)
      throw error
    }
    try {
      const bridge = await this.bridge
      const session =
        operation === 'set'
          ? await bridge.setValue(canonicalJson, identity, current.session, fieldId, value!)
          : await bridge.clearValue(canonicalJson, identity, current.session, fieldId)
      if (!sameIdentity(this.snapshotValue.identity, identity)) {
        throw new FormSessionBridgeError('FLOW_FORM_SESSION_SOURCE_CHANGED')
      }
      this.publish({ phase: 'ready', identity, session, errorCode: null })
    } catch (error: unknown) {
      if (sameIdentity(this.snapshotValue.identity, identity)) {
        this.publishError(identity, sessionErrorCode(error), current.session)
      }
      throw error
    }
  }

  private publishError(
    identity: FormSessionIdentityDto,
    errorCode: string,
    session: FormSessionStateDto | null,
  ): void {
    this.publish({ phase: 'error', identity, session, errorCode })
  }

  private publish(snapshot: FormSessionCoordinatorSnapshot): void {
    this.snapshotValue = Object.freeze(snapshot)
    for (const listener of this.listeners) listener()
  }
}

function isTransportSession(value: unknown): value is FormSessionStateDto {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return false
  const candidate = value as Partial<FormSessionStateDto>
  return (
    candidate.schemaVersion === 1 &&
    typeof candidate.documentId === 'string' &&
    Number.isSafeInteger(candidate.sourceRevision) &&
    (candidate.sourceRevision ?? -1) >= 0 &&
    typeof candidate.sourceHash === 'string' &&
    Number.isSafeInteger(candidate.generation) &&
    (candidate.generation ?? -1) >= 0 &&
    candidate.overrides !== null &&
    typeof candidate.overrides === 'object' &&
    !Array.isArray(candidate.overrides)
  )
}

function sameIdentity(
  left: FormSessionIdentityDto | null,
  right: FormSessionIdentityDto | null,
): boolean {
  if (left === null || right === null) return left === right
  return (
    left.documentId === right.documentId &&
    left.sourceRevision === right.sourceRevision &&
    left.sourceHash === right.sourceHash
  )
}

function sessionErrorCode(error: unknown): string {
  if (error instanceof FormSessionBridgeError) return error.code
  if (error !== null && typeof error === 'object') {
    const code = (error as { readonly code?: unknown }).code
    if (typeof code === 'string' && code.length > 0) return code
  }
  return 'FLOW_FORM_SESSION_UNEXPECTED'
}
