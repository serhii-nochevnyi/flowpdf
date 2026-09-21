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

export class FormSessionBridgeError extends Error {
  constructor(readonly code: string) {
    super(code)
    this.name = 'FormSessionBridgeError'
  }
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
  left: FormSessionIdentityDto,
  right: FormSessionIdentityDto,
): boolean {
  return (
    left.documentId === right.documentId &&
    left.sourceRevision === right.sourceRevision &&
    left.sourceHash === right.sourceHash
  )
}
