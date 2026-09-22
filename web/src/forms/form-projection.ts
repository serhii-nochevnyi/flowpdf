import type {
  FormSessionStateDto,
  FormSessionValueDto,
} from '../../persistence/indexeddb-store.js'
import type { FieldKindDto } from '../editor/editor-store.js'
import {
  validateLayoutRequest,
  type AcceptedLayoutDto,
  type LayoutRequestDto,
} from '../layout/layout-protocol.js'

export const FORM_PROJECTION_PROTOCOL_VERSION = 1 as const
export const FORM_PROJECTION_SELECTION_PROTOCOL_VERSION = 1 as const

export interface FormProjectionRectDto {
  readonly x: number
  readonly y: number
  readonly width: number
  readonly height: number
}

export type FormWidgetReviewReasonDto =
  | { readonly kind: 'legacyInvalid'; readonly reason: 'nonGraphemeBoundary' | 'missingNode' }
  | { readonly kind: 'targetDeleted' }
  | { readonly kind: 'targetMissing' }
  | { readonly kind: 'positionInvalid' }
  | { readonly kind: 'sourceMappingMissing' }

export interface FormWidgetReviewDto {
  readonly fieldId: string
  readonly sourceNodeId: string
  readonly reason: FormWidgetReviewReasonDto
}

export interface FormWidgetDto {
  readonly widgetId: string
  readonly fieldId: string
  readonly name: string
  readonly label: string | null
  readonly kind: FieldKindDto
  readonly required: boolean
  readonly readOnly: boolean
  readonly defaultValue: FormSessionValueDto
  readonly value: FormSessionValueDto
  readonly pageIndex: number
  readonly rect: FormProjectionRectDto
  readonly sourceNodeId: string
  readonly anchorOffsetUtf16: number
  readonly tabOrder: number
}

export interface FormWidgetProjectionDto {
  readonly schemaVersion: number
  readonly sourceRevision: number
  readonly sourceHash: string
  readonly displayListHash: string
  readonly widgets: readonly FormWidgetDto[]
  readonly review: readonly FormWidgetReviewDto[]
  readonly resultHash: string
}

export type PdfFormFieldTypeDto = 'text' | 'button' | 'choice' | 'signature'

export type PdfFormValueDto =
  | { readonly type: 'empty' }
  | { readonly type: 'text'; readonly value: string }
  | { readonly type: 'name'; readonly value: string }
  | { readonly type: 'names'; readonly values: readonly string[] }

export interface PdfFormOptionDto {
  readonly name: string
  readonly label: string
  readonly exportValue: string
}

export interface PdfFormFieldDto {
  readonly fieldId: string
  readonly widgetId: string
  readonly name: string
  readonly label: string | null
  readonly fieldType: PdfFormFieldTypeDto
  readonly flags: number
  readonly defaultValue: PdfFormValueDto
  readonly value: PdfFormValueDto
  readonly tabOrder: number
  readonly options: readonly PdfFormOptionDto[]
  readonly pageIndex: number
  readonly rect: FormProjectionRectDto
}

export interface PdfFormPlanDto {
  readonly schemaVersion: number
  readonly sourceRevision: number
  readonly sourceHash: string
  readonly displayListHash: string
  readonly fields: readonly PdfFormFieldDto[]
  readonly resultHash: string
}

/** The browser request is derived from one accepted layout request. */
export interface FormProjectionRequestDto {
  readonly protocolVersion: typeof FORM_PROJECTION_PROTOCOL_VERSION
  readonly requestId: string
  readonly sourceRevision: number
  readonly sourceHash: string
  readonly layoutResultHash: string
  readonly layout: LayoutRequestDto
  readonly session: FormSessionStateDto | null
}

export interface FormProjectionResultDto {
  readonly protocolVersion: typeof FORM_PROJECTION_PROTOCOL_VERSION
  readonly requestId: string
  readonly sourceRevision: number
  readonly sourceHash: string
  readonly layoutResultHash: string
  readonly displayListHash: string
  readonly projection: FormWidgetProjectionDto
  readonly formPlan: PdfFormPlanDto | null
}

/** Source-bound UI intent for the next PDF export. Rust remains authoritative. */
export interface FormProjectionSelectionDto {
  readonly protocolVersion: typeof FORM_PROJECTION_SELECTION_PROTOCOL_VERSION
  readonly sourceRevision: number
  readonly sourceHash: string
  readonly displayListHash: string
  readonly formPlanResultHash: string
  readonly formPlan: PdfFormPlanDto
  readonly flattenedFieldIds: readonly string[]
}

export interface AcceptedFormProjectionDto {
  readonly request: FormProjectionRequestDto
  readonly result: FormProjectionResultDto
}

export type FormProjectionSchedulerPhase = 'idle' | 'pending' | 'ready' | 'error'

export interface FormProjectionSchedulerSnapshotDto {
  readonly phase: FormProjectionSchedulerPhase
  readonly accepted: AcceptedFormProjectionDto | null
  readonly requestId: string | null
  readonly errorCode: string | null
}

export interface FormProjectionWorkerDiagnosticDto {
  readonly requestId: string
  readonly sourceRevision: number
  readonly code: string
}

export interface FormProjectionWasmBoundary {
  readonly project_form_widgets: (requestJson: string) => string
  readonly verify_form_projection_response: (responseJson: string) => boolean
}

export interface OptionalFormProjectionWasmBoundary {
  readonly project_form_widgets?: FormProjectionWasmBoundary['project_form_widgets']
  readonly verify_form_projection_response?:
    FormProjectionWasmBoundary['verify_form_projection_response']
}

export function requireFormProjectionWasm(
  boundary: OptionalFormProjectionWasmBoundary,
): FormProjectionWasmBoundary {
  if (
    boundary.project_form_widgets === undefined ||
    boundary.verify_form_projection_response === undefined
  ) {
    throw new FormProjectionWorkerError('FLOW_FORM_PROJECTION_UNAVAILABLE')
  }
  return {
    project_form_widgets: boundary.project_form_widgets,
    verify_form_projection_response: boundary.verify_form_projection_response,
  }
}

export interface FormProjectionEngine {
  (
    request: FormProjectionRequestDto,
    signal: AbortSignal,
  ): Promise<FormProjectionResultDto>
}

export interface FormProjectionEngineAdapter {
  readonly run: FormProjectionEngine
  readonly verifyResultHash: (
    result: FormProjectionResultDto,
  ) => boolean | Promise<boolean>
}

export interface FormProjectionSchedulerOptions {
  readonly verifyResultHash: (
    result: FormProjectionResultDto,
  ) => boolean | Promise<boolean>
  readonly onPublished?: (accepted: AcceptedFormProjectionDto) => void
  readonly onDiagnostic?: (diagnostic: FormProjectionWorkerDiagnosticDto) => void
}

export interface FormProjectionScheduler {
  request(request: FormProjectionRequestDto): Promise<FormProjectionScheduleOutcome>
  cancel(requestId?: string): void
  accepted(): AcceptedFormProjectionDto | null
  snapshot(): FormProjectionSchedulerSnapshotDto
  subscribe(listener: () => void): () => void
  dispose?(): void
}

export type FormProjectionScheduleOutcome =
  | { readonly kind: 'published'; readonly accepted: AcceptedFormProjectionDto }
  | { readonly kind: 'discarded'; readonly requestId: string; readonly code: string }
  | { readonly kind: 'cancelled'; readonly requestId: string }
  | { readonly kind: 'failed'; readonly requestId: string; readonly code: string }

const MAX_REQUEST_ID_CHARS = 128
const MAX_SERIALIZED_REQUEST_CHARS = 96 * 1024 * 1024

export class FormProjectionWorkerError extends Error {
  constructor(readonly code: string) {
    super(code)
    this.name = 'FormProjectionWorkerError'
  }
}

/** Binds a projection request to the exact layout result accepted by Rust. */
export function createFormProjectionRequest(
  acceptedLayout: AcceptedLayoutDto,
  requestId: string,
  session: FormSessionStateDto | null,
): FormProjectionRequestDto {
  return {
    protocolVersion: FORM_PROJECTION_PROTOCOL_VERSION,
    requestId,
    sourceRevision: acceptedLayout.result.sourceRevision,
    sourceHash: acceptedLayout.result.sourceHash,
    layoutResultHash: acceptedLayout.result.resultHash,
    layout: acceptedLayout.request,
    session,
  }
}

export function validateFormProjectionRequest(
  request: FormProjectionRequestDto,
): string | null {
  if (request.protocolVersion !== FORM_PROJECTION_PROTOCOL_VERSION) {
    return 'FLOW_FORM_PROJECTION_PROTOCOL_VERSION'
  }
  if (
    request.requestId.trim().length === 0 ||
    request.requestId.length > MAX_REQUEST_ID_CHARS
  ) {
    return 'FLOW_FORM_PROJECTION_REQUEST_ID_INVALID'
  }
  if (!Number.isSafeInteger(request.sourceRevision) || request.sourceRevision < 0) {
    return 'FLOW_FORM_PROJECTION_SOURCE_REVISION_INVALID'
  }
  if (request.sourceHash.trim().length === 0) {
    return 'FLOW_FORM_PROJECTION_SOURCE_HASH_INVALID'
  }
  if (request.layoutResultHash.trim().length === 0) {
    return 'FLOW_FORM_PROJECTION_LAYOUT_RESULT_HASH_INVALID'
  }
  const layoutError = validateLayoutRequest(request.layout)
  if (layoutError !== null) return `FLOW_FORM_PROJECTION_${layoutError.slice('FLOW_LAYOUT_'.length)}`
  if (
    request.layout.sourceRevision !== request.sourceRevision ||
    request.layout.sourceHash !== request.sourceHash
  ) {
    return 'FLOW_FORM_PROJECTION_LAYOUT_SOURCE_MISMATCH'
  }
  if (request.layout.serializedRequest.length > MAX_SERIALIZED_REQUEST_CHARS) {
    return 'FLOW_FORM_PROJECTION_LAYOUT_REQUEST_LIMIT'
  }
  if (request.session !== null && !isFormSessionState(request.session)) {
    return 'FLOW_FORM_PROJECTION_SESSION_INVALID'
  }
  if (
    request.session !== null &&
    (request.session.sourceRevision !== request.sourceRevision ||
      request.session.sourceHash !== request.sourceHash)
  ) {
    return 'FLOW_FORM_PROJECTION_SESSION_SOURCE_MISMATCH'
  }
  return null
}

export function validateFormProjectionResult(
  request: FormProjectionRequestDto,
  result: FormProjectionResultDto,
): string | null {
  if (result.protocolVersion !== FORM_PROJECTION_PROTOCOL_VERSION) {
    return 'FLOW_FORM_PROJECTION_PROTOCOL_VERSION'
  }
  if (result.requestId !== request.requestId) {
    return 'FLOW_FORM_PROJECTION_STALE_REQUEST_ID'
  }
  if (result.sourceRevision !== request.sourceRevision) {
    return 'FLOW_FORM_PROJECTION_STALE_SOURCE_REVISION'
  }
  if (result.sourceHash !== request.sourceHash) {
    return 'FLOW_FORM_PROJECTION_STALE_SOURCE_HASH'
  }
  if (result.layoutResultHash !== request.layoutResultHash) {
    return 'FLOW_FORM_PROJECTION_STALE_LAYOUT_RESULT'
  }
  if (
    result.projection.sourceRevision !== result.sourceRevision ||
    result.projection.sourceHash !== result.sourceHash ||
    result.projection.displayListHash !== result.displayListHash ||
    result.projection.resultHash.trim().length === 0
  ) {
    return 'FLOW_FORM_PROJECTION_RESULT_IDENTITY_INVALID'
  }
  if (result.formPlan !== null) {
    if (
      result.projection.review.length > 0 ||
      result.formPlan.sourceRevision !== result.sourceRevision ||
      result.formPlan.sourceHash !== result.sourceHash ||
      result.formPlan.displayListHash !== result.displayListHash ||
      result.formPlan.resultHash.trim().length === 0
    ) {
      return 'FLOW_FORM_PROJECTION_PLAN_IDENTITY_INVALID'
    }
  }
  return null
}

/**
 * Validates a browser selection without accepting any browser-authored plan
 * fields. The plan itself must have come from a verified Rust projection.
 */
export function validateFormProjectionSelection(
  selection: FormProjectionSelectionDto,
): string | null {
  if (selection === null || typeof selection !== 'object') {
    return 'FLOW_FORM_PROJECTION_SELECTION_INVALID'
  }
  if (selection.protocolVersion !== FORM_PROJECTION_SELECTION_PROTOCOL_VERSION) {
    return 'FLOW_FORM_PROJECTION_SELECTION_PROTOCOL_VERSION'
  }
  if (!Number.isSafeInteger(selection.sourceRevision) || selection.sourceRevision < 0) {
    return 'FLOW_FORM_PROJECTION_SELECTION_SOURCE_REVISION_INVALID'
  }
  if (typeof selection.sourceHash !== 'string' || selection.sourceHash.trim().length === 0) {
    return 'FLOW_FORM_PROJECTION_SELECTION_SOURCE_HASH_INVALID'
  }
  if (
    typeof selection.displayListHash !== 'string' ||
    selection.displayListHash.trim().length === 0
  ) {
    return 'FLOW_FORM_PROJECTION_SELECTION_DISPLAY_LIST_HASH_INVALID'
  }
  const plan = selection.formPlan
  if (plan === null || typeof plan !== 'object' || !Array.isArray(plan.fields)) {
    return 'FLOW_FORM_PROJECTION_SELECTION_PLAN_INVALID'
  }
  if (
    plan.sourceRevision !== selection.sourceRevision ||
    typeof plan.sourceHash !== 'string' ||
    plan.sourceHash !== selection.sourceHash ||
    typeof plan.displayListHash !== 'string' ||
    plan.displayListHash !== selection.displayListHash ||
    typeof plan.resultHash !== 'string' ||
    plan.resultHash !== selection.formPlanResultHash ||
    plan.resultHash.trim().length === 0
  ) {
    return 'FLOW_FORM_PROJECTION_SELECTION_PLAN_IDENTITY_INVALID'
  }
  const planFieldIds = new Set<string>()
  for (const field of plan.fields) {
    if (
      typeof field.fieldId !== 'string' ||
      field.fieldId.trim().length === 0 ||
      planFieldIds.has(field.fieldId)
    ) {
      return 'FLOW_FORM_PROJECTION_SELECTION_PLAN_INVALID'
    }
    planFieldIds.add(field.fieldId)
  }
  const selectedFieldIds = new Set<string>()
  if (!Array.isArray(selection.flattenedFieldIds)) {
    return 'FLOW_FORM_PROJECTION_SELECTION_FIELD_ID_INVALID'
  }
  for (const fieldId of selection.flattenedFieldIds) {
    if (typeof fieldId !== 'string' || fieldId.trim().length === 0) {
      return 'FLOW_FORM_PROJECTION_SELECTION_FIELD_ID_INVALID'
    }
    if (selectedFieldIds.has(fieldId)) {
      return 'FLOW_FORM_PROJECTION_SELECTION_FIELD_ID_DUPLICATE'
    }
    if (!planFieldIds.has(fieldId)) {
      return 'FLOW_FORM_PROJECTION_SELECTION_FIELD_ID_UNKNOWN'
    }
    selectedFieldIds.add(fieldId)
  }
  return null
}

/**
 * Creates a normalized source-bound selection from a verified projection.
 * Unknown, duplicate, or malformed IDs fail closed instead of being silently
 * converted into an export option.
 */
export function createFormProjectionSelection(
  result: FormProjectionResultDto | null,
  selectedFieldIds: readonly string[],
): FormProjectionSelectionDto | null {
  if (
    result === null ||
    result.formPlan === null ||
    result.projection.review.length > 0 ||
    !Array.isArray(selectedFieldIds)
  ) {
    return null
  }
  const selected = new Set<string>()
  for (const fieldId of selectedFieldIds) {
    if (typeof fieldId !== 'string' || selected.has(fieldId)) return null
    selected.add(fieldId)
  }
  const flattenedFieldIds = result.formPlan.fields
    .filter((field) => selected.has(field.fieldId))
    .map((field) => field.fieldId)
  if (flattenedFieldIds.length !== selected.size) return null
  const selection: FormProjectionSelectionDto = {
    protocolVersion: FORM_PROJECTION_SELECTION_PROTOCOL_VERSION,
    sourceRevision: result.sourceRevision,
    sourceHash: result.sourceHash,
    displayListHash: result.displayListHash,
    formPlanResultHash: result.formPlan.resultHash,
    formPlan: result.formPlan,
    flattenedFieldIds,
  }
  return validateFormProjectionSelection(selection) === null ? selection : null
}

/** Adapts Rust's closed string-only projection boundary to a scheduler engine. */
export function createWasmFormProjectionEngine(
  wasm: FormProjectionWasmBoundary,
): FormProjectionEngineAdapter {
  const run: FormProjectionEngine = async (request, signal) => {
    if (signal.aborted) throw new FormProjectionWorkerError('FLOW_FORM_PROJECTION_CANCELLED')
    let serializedRequest: string
    try {
      serializedRequest = JSON.stringify({
        schemaVersion: FORM_PROJECTION_PROTOCOL_VERSION,
        requestId: request.requestId,
        layoutRequestJson: JSON.stringify(request.layout),
        session: request.session,
      })
    } catch {
      throw new FormProjectionWorkerError('FLOW_FORM_PROJECTION_REQUEST_ENCODE')
    }
    if (serializedRequest.length > MAX_SERIALIZED_REQUEST_CHARS) {
      throw new FormProjectionWorkerError('FLOW_FORM_PROJECTION_REQUEST_LIMIT')
    }
    let serializedResponse: string
    try {
      serializedResponse = wasm.project_form_widgets(serializedRequest)
    } catch {
      throw new FormProjectionWorkerError('FLOW_FORM_PROJECTION_BOUNDARY_FAILED')
    }
    if (signal.aborted) throw new FormProjectionWorkerError('FLOW_FORM_PROJECTION_CANCELLED')
    let parsed: unknown
    try {
      parsed = JSON.parse(serializedResponse) as unknown
    } catch {
      throw new FormProjectionWorkerError('FLOW_FORM_PROJECTION_RESPONSE_DECODE')
    }
    if (!isRustFormProjectionResponse(parsed)) {
      throw new FormProjectionWorkerError('FLOW_FORM_PROJECTION_RESPONSE_DECODE')
    }
    if (!parsed.ok || parsed.result === null) {
      throw new FormProjectionWorkerError(
        parsed.error?.code ?? 'FLOW_FORM_PROJECTION_UNKNOWN_ERROR',
      )
    }
    if (!wasm.verify_form_projection_response(serializedResponse)) {
      throw new FormProjectionWorkerError('FLOW_FORM_PROJECTION_RESULT_HASH_INVALID')
    }
    if (!isFormProjectionResult(parsed.result)) {
      throw new FormProjectionWorkerError('FLOW_FORM_PROJECTION_RESPONSE_DECODE')
    }
    if (
      parsed.sourceRevision !== parsed.result.sourceRevision ||
      parsed.sourceHash !== parsed.result.sourceHash ||
      parsed.layoutResultHash !== parsed.result.layoutResultHash ||
      parsed.displayListHash !== parsed.result.displayListHash ||
      parsed.resultHash === null ||
      parsed.resultHash.trim().length === 0
    ) {
      throw new FormProjectionWorkerError('FLOW_FORM_PROJECTION_RESULT_IDENTITY_INVALID')
    }
    const result: FormProjectionResultDto = {
      protocolVersion: FORM_PROJECTION_PROTOCOL_VERSION,
      requestId: parsed.requestId,
      ...parsed.result,
    }
    const mismatch = validateFormProjectionResult(request, result)
    if (mismatch !== null) throw new FormProjectionWorkerError(mismatch)
    return result
  }
  return {
    run,
    verifyResultHash: (result) =>
      result.projection.resultHash.trim().length > 0 &&
      (result.formPlan === null || result.formPlan.resultHash.trim().length > 0),
  }
}

/** Revision-aware single-flight publication for derived form projection. */
export class RevisionAwareFormProjectionScheduler {
  private readonly listeners = new Set<() => void>()
  private generation = 0
  private activeRequest: {
    readonly generation: number
    readonly requestId: string
    readonly controller: AbortController
  } | null = null
  private acceptedValue: AcceptedFormProjectionDto | null = null
  private phase: FormProjectionSchedulerPhase = 'idle'
  private pendingRequestId: string | null = null
  private errorValue: string | null = null
  private snapshotValue: FormProjectionSchedulerSnapshotDto = Object.freeze({
    phase: 'idle',
    accepted: null,
    requestId: null,
    errorCode: null,
  })

  constructor(
    private readonly engine: FormProjectionEngine,
    private readonly options: FormProjectionSchedulerOptions,
  ) {}

  accepted(): AcceptedFormProjectionDto | null {
    return this.acceptedValue
  }

  readonly snapshot = (): FormProjectionSchedulerSnapshotDto => this.snapshotValue

  subscribe(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => this.listeners.delete(listener)
  }

  request(request: FormProjectionRequestDto): Promise<FormProjectionScheduleOutcome> {
    const requestError = validateFormProjectionRequest(request)
    if (requestError !== null) {
      this.finishWithError(requestError)
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
    request: FormProjectionRequestDto,
    generation: number,
    controller: AbortController,
  ): Promise<FormProjectionScheduleOutcome> {
    try {
      const result = await this.engine(request, controller.signal)
      if (controller.signal.aborted) {
        return { kind: 'cancelled', requestId: request.requestId }
      }
      if (!this.isCurrent(generation, request.requestId)) {
        return {
          kind: 'discarded',
          requestId: request.requestId,
          code: 'FLOW_FORM_PROJECTION_STALE_REQUEST',
        }
      }
      const mismatch = validateFormProjectionResult(request, result)
      if (mismatch !== null) {
        this.finishWithError(mismatch)
        this.report(request, mismatch)
        return { kind: 'discarded', requestId: request.requestId, code: mismatch }
      }
      if (!(await this.options.verifyResultHash(result))) {
        this.finishWithError('FLOW_FORM_PROJECTION_RESULT_HASH_INVALID')
        this.report(request, 'FLOW_FORM_PROJECTION_RESULT_HASH_INVALID')
        return {
          kind: 'discarded',
          requestId: request.requestId,
          code: 'FLOW_FORM_PROJECTION_RESULT_HASH_INVALID',
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
      if (controller.signal.aborted) {
        return { kind: 'cancelled', requestId: request.requestId }
      }
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

  private report(request: FormProjectionRequestDto, code: string): void {
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
    this.snapshotValue = Object.freeze({
      phase: this.phase,
      accepted: this.acceptedValue,
      requestId: this.pendingRequestId,
      errorCode: this.errorValue,
    })
    for (const listener of this.listeners) listener()
  }
}

interface RustFormProjectionResponse {
  readonly schemaVersion: number
  readonly requestId: string
  readonly ok: boolean
  readonly sourceRevision: number | null
  readonly sourceHash: string | null
  readonly layoutResultHash: string | null
  readonly displayListHash: string | null
  readonly resultHash: string | null
  readonly result: Omit<FormProjectionResultDto, 'protocolVersion' | 'requestId'> | null
  readonly error: { readonly code: string } | null
}

function errorCode(error: unknown): string {
  if (error instanceof FormProjectionWorkerError) return error.code
  return 'FLOW_FORM_PROJECTION_WORKER_FAILURE'
}

function isFormSessionState(value: unknown): value is FormSessionStateDto {
  if (!isRecord(value)) return false
  return (
    value.schemaVersion === 1 &&
    typeof value.documentId === 'string' &&
    isNonNegativeSafeInteger(value.sourceRevision) &&
    typeof value.sourceHash === 'string' &&
    isNonNegativeSafeInteger(value.generation) &&
    isRecord(value.overrides)
  )
}

function isRustFormProjectionResponse(value: unknown): value is RustFormProjectionResponse {
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
  if (!value.ok || value.result === null) return true
  return (
    isNonNegativeSafeInteger(value.sourceRevision) &&
    typeof value.sourceHash === 'string' &&
    typeof value.layoutResultHash === 'string' &&
    typeof value.displayListHash === 'string' &&
    typeof value.resultHash === 'string' &&
    isFormProjectionResult(value.result)
  )
}

function isFormProjectionResult(value: unknown): value is Omit<
  FormProjectionResultDto,
  'protocolVersion' | 'requestId'
> {
  if (!isRecord(value)) return false
  return (
    isNonNegativeSafeInteger(value.sourceRevision) &&
    typeof value.sourceHash === 'string' &&
    typeof value.layoutResultHash === 'string' &&
    typeof value.displayListHash === 'string' &&
    isFormWidgetProjection(value.projection) &&
    (value.formPlan === null || isPdfFormPlan(value.formPlan))
  )
}

function isFormWidgetProjection(value: unknown): value is FormWidgetProjectionDto {
  if (!isRecord(value)) return false
  return (
    isNonNegativeSafeInteger(value.schemaVersion) &&
    isNonNegativeSafeInteger(value.sourceRevision) &&
    typeof value.sourceHash === 'string' &&
    typeof value.displayListHash === 'string' &&
    Array.isArray(value.widgets) &&
    value.widgets.every(isFormWidget) &&
    Array.isArray(value.review) &&
    value.review.every(isFormWidgetReview) &&
    typeof value.resultHash === 'string'
  )
}

function isFormWidget(value: unknown): value is FormWidgetDto {
  if (!isRecord(value)) return false
  return (
    typeof value.widgetId === 'string' &&
    typeof value.fieldId === 'string' &&
    typeof value.name === 'string' &&
    (value.label === null || typeof value.label === 'string') &&
    isFieldKind(value.kind) &&
    typeof value.required === 'boolean' &&
    typeof value.readOnly === 'boolean' &&
    isFormSessionValue(value.defaultValue) &&
    isFormSessionValue(value.value) &&
    isNonNegativeSafeInteger(value.pageIndex) &&
    isRect(value.rect) &&
    typeof value.sourceNodeId === 'string' &&
    isNonNegativeSafeInteger(value.anchorOffsetUtf16) &&
    isNonNegativeSafeInteger(value.tabOrder)
  )
}

function isFormWidgetReview(value: unknown): value is FormWidgetReviewDto {
  if (!isRecord(value) || typeof value.fieldId !== 'string' || typeof value.sourceNodeId !== 'string') {
    return false
  }
  if (!isRecord(value.reason) || typeof value.reason.kind !== 'string') return false
  if (value.reason.kind === 'legacyInvalid') {
    return value.reason.reason === 'nonGraphemeBoundary' || value.reason.reason === 'missingNode'
  }
  return (
    value.reason.kind === 'targetDeleted' ||
    value.reason.kind === 'targetMissing' ||
    value.reason.kind === 'positionInvalid' ||
    value.reason.kind === 'sourceMappingMissing'
  )
}

function isPdfFormPlan(value: unknown): value is PdfFormPlanDto {
  if (!isRecord(value)) return false
  return (
    isNonNegativeSafeInteger(value.schemaVersion) &&
    isNonNegativeSafeInteger(value.sourceRevision) &&
    typeof value.sourceHash === 'string' &&
    typeof value.displayListHash === 'string' &&
    Array.isArray(value.fields) &&
    value.fields.every(isPdfFormField) &&
    typeof value.resultHash === 'string'
  )
}

function isPdfFormField(value: unknown): value is PdfFormFieldDto {
  if (!isRecord(value)) return false
  return (
    typeof value.fieldId === 'string' &&
    typeof value.widgetId === 'string' &&
    typeof value.name === 'string' &&
    (value.label === null || typeof value.label === 'string') &&
    (value.fieldType === 'text' ||
      value.fieldType === 'button' ||
      value.fieldType === 'choice' ||
      value.fieldType === 'signature') &&
    isNonNegativeSafeInteger(value.flags) &&
    isPdfFormValue(value.defaultValue) &&
    isPdfFormValue(value.value) &&
    isNonNegativeSafeInteger(value.tabOrder) &&
    Array.isArray(value.options) &&
    value.options.every(isPdfFormOption) &&
    isNonNegativeSafeInteger(value.pageIndex) &&
    isRect(value.rect)
  )
}

function isPdfFormOption(value: unknown): value is PdfFormOptionDto {
  return (
    isRecord(value) &&
    typeof value.name === 'string' &&
    typeof value.label === 'string' &&
    typeof value.exportValue === 'string'
  )
}

function isFormSessionValue(value: unknown): value is FormSessionValueDto {
  if (!isRecord(value) || typeof value.type !== 'string') return false
  if (value.type === 'empty') return true
  if (value.type === 'text') return typeof value.value === 'string'
  if (value.type === 'checked') return typeof value.value === 'boolean'
  return (
    value.type === 'selected' &&
    Array.isArray(value.optionIds) &&
    value.optionIds.every((optionId) => typeof optionId === 'string')
  )
}

function isPdfFormValue(value: unknown): value is PdfFormValueDto {
  if (!isRecord(value) || typeof value.type !== 'string') return false
  if (value.type === 'empty') return true
  if (value.type === 'text' || value.type === 'name') {
    return typeof value.value === 'string'
  }
  return (
    value.type === 'names' &&
    Array.isArray(value.values) &&
    value.values.every((item) => typeof item === 'string')
  )
}

function isFieldKind(value: unknown): value is FieldKindDto {
  if (!isRecord(value) || typeof value.type !== 'string') return false
  if (value.type === 'text') {
    return (
      typeof value.multiline === 'boolean' &&
      (value.inputHint === 'plain' ||
        value.inputHint === 'date' ||
        value.inputHint === 'number' ||
        value.inputHint === 'email')
    )
  }
  if (value.type === 'select') return typeof value.multiple === 'boolean'
  return (
    value.type === 'checkbox' ||
    value.type === 'radioGroup' ||
    value.type === 'signature' ||
    value.type === 'button'
  )
}

function isRect(value: unknown): value is FormProjectionRectDto {
  return (
    isRecord(value) &&
    typeof value.x === 'number' &&
    Number.isSafeInteger(value.x) &&
    typeof value.y === 'number' &&
    Number.isSafeInteger(value.y) &&
    typeof value.width === 'number' &&
    Number.isSafeInteger(value.width) &&
    typeof value.height === 'number' &&
    Number.isSafeInteger(value.height)
  )
}

function isNonNegativeSafeInteger(value: unknown): value is number {
  return typeof value === 'number' && Number.isSafeInteger(value) && value >= 0
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}
